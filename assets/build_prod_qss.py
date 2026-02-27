#!/usr/bin/env python3
"""
Build script to pre-process QSS files for distribution.

This script processes all QSS files in the assets directory and generates
processed versions in assets/processed/ that can be included in distributions
to avoid runtime preprocessing overhead.
"""

import os
import sys

# Add the assets directory to the path so we can import the preprocessor
sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'assets'))

from qss_variables_preprocessor import process_qss_files

def main():
    """Main build function."""
    assets_dir = "assets"
    processed_dir = os.path.join(assets_dir, "processed")

    print("Building QSS files...")
    print(f"Source directory: {assets_dir}")
    print(f"Output directory: {processed_dir}")

    # Process all QSS files
    process_qss_files(assets_dir, processed_dir)

    print("Build complete!")

if __name__ == "__main__":
    main()