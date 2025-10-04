import os
import hashlib
import json
import sys
import argparse
from pathlib import Path
from zipfile import ZipFile

try:
    import bsdiff4
except ImportError:
    print("bsdiff4 is not installed. Please install it with 'pip install bsdiff4'")
    sys.exit(1)

def get_sha256(file_path):
    """Calculates the SHA256 hash of a file."""
    sha256 = hashlib.sha256()
    with open(file_path, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256.update(byte_block)
    return sha256.hexdigest()

def generate_manifest(directory, exclude_prefix="torch/"):
    """Generates a manifest of file paths to their SHA256 hashes."""
    manifest = {"files": {}}
    for path in Path(directory).rglob("*"):
        if path.is_file() and not str(path.relative_to(directory)).startswith(exclude_prefix):
            file_key = str(path.relative_to(directory)).replace("\\", "/")
            manifest["files"][file_key] = get_sha256(path)
    return manifest

def main():
    parser = argparse.ArgumentParser(description="Create a differential update package for an application.")
    parser.add_argument("--build-dir", type=Path, default=Path("current-build"), help="Directory of the current build.")
    parser.add_argument("--old-manifest", type=Path, help="Path to the manifest.json from the previous release.")
    parser.add_argument("--old-executable", type=Path, help="Path to the main executable from the previous release.")
    parser.add_argument("--main-executable-name", type=str, default="main.exe", help="Name of the main executable file.")
    args = parser.parse_args()

    build_dir = args.build_dir
    main_executable = args.main_executable_name
    old_executable_path = args.old_executable
    old_manifest_path = args.old_manifest

    new_manifest_file = Path("manifest.json")
    update_package_file = Path("update-package.zip")
    patch_file = Path(f"{main_executable}.patch")

    print("Generating manifest for the current build...")
    new_manifest = generate_manifest(build_dir)
    with open(new_manifest_file, "w") as f:
        json.dump(new_manifest, f, indent=4)
    print(f"New manifest created at {new_manifest_file}")

    old_manifest = {}
    if old_manifest_path and old_manifest_path.exists():
        print(f"Previous manifest found at '{old_manifest_path}'. Loading for comparison.")
        with open(old_manifest_path, "r") as f:
            old_manifest = json.load(f)
    else:
        print("No previous manifest found. A full package will be created.")

    # Compare manifests
    files_to_package = []
    if not old_manifest.get("files"):
        files_to_package = list(new_manifest.get("files", {}).keys())
        print("Creating a full update package.")
    else:
        for file, new_hash in new_manifest.get("files", {}).items():
            if file not in old_manifest.get("files", {}) or old_manifest["files"][file] != new_hash:
                files_to_package.append(file)
        print(f"Found {len(files_to_package)} new or modified files.")

    # Determine if we can create a patch for the main executable
    can_create_patch = (
        main_executable in files_to_package and
        old_manifest.get("files") and
        old_executable_path and
        old_executable_path.exists()
    )

    if can_create_patch:
        print(f"Old executable found. Attempting to create a patch for {main_executable}...")
        bsdiff4.file_diff(old_executable_path, build_dir / main_executable, patch_file)
        print(f"Patch created at {patch_file}")

        new_manifest["patch"] = {
            "file": main_executable,
            "patch_file": patch_file.name,
            "old_sha256": get_sha256(old_executable_path)
        }

        files_to_package.remove(main_executable)
        files_to_package.append(patch_file.name)
    elif main_executable in files_to_package:
        if not old_executable_path or not old_executable_path.exists():
            print(f"Previous main executable not found. This is likely the first release introducing bsdiff updates.")
            print("Skipping patch generation and including the full executable in the package.")
        else:
            print(f"Old executable provided but other conditions not met. '{main_executable}' will be included as a full file.")


    if not files_to_package:
        print("No file changes detected. Update package will not be created.")
        if 'GITHUB_OUTPUT' in os.environ:
            with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
                print("package_created=false", file=f)
        return

    # Rewrite the manifest with patch info if it was added
    with open(new_manifest_file, "w") as f:
        json.dump(new_manifest, f, indent=4)

    print(f"Creating update package '{update_package_file}'...")
    with ZipFile(update_package_file, "w") as zipf:
        # First, add the manifest to the package
        zipf.write(new_manifest_file, arcname=new_manifest_file.name)
        # Then, add all other files
        for file in files_to_package:
            source_path = patch_file if file == patch_file.name else build_dir / file
            zipf.write(source_path, arcname=file)

    print("Update package created successfully.")
    if 'GITHUB_OUTPUT' in os.environ:
        with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
            print("package_created=true", file=f)

if __name__ == "__main__":
    main()