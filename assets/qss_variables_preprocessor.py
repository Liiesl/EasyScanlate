#!/usr/bin/env python3
"""
QSS Variables Preprocessor

This module provides functionality to preprocess QSS files by replacing
variable references with their actual values. Variables are defined in
separate .qssvars files using the format: $variable = value;

Usage:
    # At runtime (for development)
    processed_qss = preprocess_qss("assets/main.qss", variables_file="assets/variables.qssvars")

    # For build (pre-generate processed files)
    process_qss_files("assets/", output_dir="assets/processed/")
"""

import os
import re
from typing import Dict, Optional


def parse_variables_file(file_path: str) -> Dict[str, str]:
    """
    Parse a .qssvars file and return a dictionary of variable name -> value mappings.

    Args:
        file_path: Path to the variables file

    Returns:
        Dictionary mapping variable names to their values
    """
    variables = {}

    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()

        # Remove :root wrapper and braces if present
        content = re.sub(r':root\s*\{', '', content)
        content = re.sub(r'\}\s*$', '', content)

        for line_num, line in enumerate(content.splitlines(), 1):
            line = line.strip()

            # Skip empty lines and comments
            if not line or line.startswith('/*') or line.startswith('//'):
                continue

            # Match variable declarations: --variable: value;
            match = re.match(r'--([a-zA-Z_][a-zA-Z0-9_]*):\s*([^;]+);', line)
            if match:
                var_name, var_value = match.groups()
                variables[var_name] = var_value.strip()
            else:
                print(f"Warning: Invalid variable declaration at line {line_num} in {file_path}: {line}")

    except FileNotFoundError:
        print(f"Error: Variables file not found: {file_path}")
        return {}
    except Exception as e:
        print(f"Error parsing variables file {file_path}: {e}")
        return {}

    return variables


def preprocess_qss(qss_content: str, variables: Dict[str, str]) -> str:
    """
    Replace all variable references in QSS content with their actual values.

    Args:
        qss_content: The QSS content as a string
        variables: Dictionary of variable name -> value mappings

    Returns:
        Processed QSS content with variables replaced
    """
    processed = qss_content

    # Replace variables in the format var(--variable)
    for var_name, var_value in variables.items():
        # Match var(--variable) and replace with the actual value
        pattern = r'var\(--' + re.escape(var_name) + r'\)'
        processed = re.sub(pattern, var_value, processed)

    return processed


def load_and_preprocess_qss(qss_file_path: str, variables_file_path: Optional[str] = None) -> str:
    """
    Load a QSS file and preprocess it by replacing variable references.

    Args:
        qss_file_path: Path to the QSS file
        variables_file_path: Path to the variables file (optional, will try to find automatically)

    Returns:
        Processed QSS content as a string
    """
    # Load QSS content
    try:
        with open(qss_file_path, 'r', encoding='utf-8') as f:
            qss_content = f.read()
    except FileNotFoundError:
        print(f"Error: QSS file not found: {qss_file_path}")
        return ""
    except Exception as e:
        print(f"Error loading QSS file {qss_file_path}: {e}")
        return ""

    # Determine variables file path
    if variables_file_path is None:
        # Try to find variables file in the same directory as the QSS file
        qss_dir = os.path.dirname(qss_file_path)
        variables_file_path = os.path.join(qss_dir, "variables.qssvars")

    # Load variables
    variables = parse_variables_file(variables_file_path)

    # Preprocess QSS
    processed_qss = preprocess_qss(qss_content, variables)

    return processed_qss


def process_qss_files(source_dir: str, output_dir: str, variables_file: Optional[str] = None) -> None:
    """
    Process all QSS files in a directory and write processed versions to output directory.

    Args:
        source_dir: Directory containing QSS files
        output_dir: Directory to write processed QSS files
        variables_file: Path to variables file (optional)
    """
    if not os.path.exists(output_dir):
        os.makedirs(output_dir)

    # Find variables file if not specified
    if variables_file is None:
        variables_file = os.path.join(source_dir, "variables.qssvars")

    # Load variables once
    variables = parse_variables_file(variables_file)

    # Process all QSS files
    for filename in os.listdir(source_dir):
        if filename.endswith('.qss'):
            qss_file_path = os.path.join(source_dir, filename)
            output_file_path = os.path.join(output_dir, filename)

            print(f"Processing {filename}...")

            try:
                with open(qss_file_path, 'r', encoding='utf-8') as f:
                    qss_content = f.read()

                processed_qss = preprocess_qss(qss_content, variables)

                with open(output_file_path, 'w', encoding='utf-8') as f:
                    f.write(processed_qss)

                print(f"  -> {output_file_path}")

            except Exception as e:
                print(f"Error processing {filename}: {e}")


if __name__ == "__main__":
    # Command line usage for preprocessing
    import sys

    if len(sys.argv) < 2:
        print("Usage:")
        print("  python qss_preprocessor.py <qss_file> [variables_file]")
        print("  python qss_preprocessor.py --process-dir <source_dir> <output_dir> [variables_file]")
        sys.exit(1)

    if sys.argv[1] == "--process-dir":
        if len(sys.argv) < 4:
            print("Usage: python qss_preprocessor.py --process-dir <source_dir> <output_dir> [variables_file]")
            sys.exit(1)

        source_dir = sys.argv[2]
        output_dir = sys.argv[3]
        variables_file = sys.argv[4] if len(sys.argv) > 4 else None

        process_qss_files(source_dir, output_dir, variables_file)
    else:
        qss_file = sys.argv[1]
        variables_file = sys.argv[2] if len(sys.argv) > 2 else None

        processed = load_and_preprocess_qss(qss_file, variables_file)
        print(processed)