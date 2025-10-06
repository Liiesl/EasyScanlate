import os
import hashlib
import json
import sys
import argparse
import subprocess
from pathlib import Path
from zipfile import ZipFile

try:
    import bsdiff4 #type: ignore
except ImportError:
    print("bsdiff4 is not installed. Please install it with 'pip install bsdiff4'")
    sys.exit(1)

# Every 3 versions, a cumulative patch is created from an older version.
HOP_INTERVAL = 3

def get_sha256(file_path):
    """Calculates the SHA256 hash of a file."""
    sha256 = hashlib.sha256()
    with open(file_path, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256.update(byte_block)
    return sha256.hexdigest()

def generate_manifest_for_build(directory, exclude_prefix="torch/"):
    """Generates a file manifest for a given build directory."""
    manifest = {}
    for path in Path(directory).rglob("*"):
        if path.is_file() and not str(path.relative_to(directory)).startswith(exclude_prefix):
            file_key = str(path.relative_to(directory)).replace("\\", "/")
            manifest[file_key] = get_sha256(path)
    return manifest

def fetch_asset(repo, tag, pattern, output_dir="."):
    """Downloads a release asset using the gh CLI."""
    print(f"Attempting to download '{pattern}' from release '{tag}'...")
    command = [
        "gh", "release", "download", tag,
        "--repo", repo,
        "--pattern", pattern,
        "--dir", output_dir
    ]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode == 0:
        print(f"Successfully downloaded '{pattern}'.")
        return Path(output_dir) / pattern
    print(f"Could not download '{pattern}' from '{tag}'. It might not exist. Error: {result.stderr.strip()}")
    return None

def get_latest_tag(repo):
    """Gets the latest release tag, including pre-releases."""
    print("Finding the latest release tag (including pre-releases)...")
    command = [
        "gh", "release", "list",
        "--repo", repo,
        "--limit", "1",
        "--json", "tagName",
        "--jq", ".[0].tagName"
    ]
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    if result.returncode == 0 and result.stdout.strip():
        tag = result.stdout.strip()
        print(f"Found latest tag: {tag}")
        return tag
    else:
        print(f"Could not find any previous releases. Error: {result.stderr.strip()}")
        return None


def create_differential_package(
    from_version, to_version,
    from_manifest, to_manifest,
    from_executable_path, to_executable_path,
    build_dir
):
    """Creates a single differential update package between two versions."""
    package_name = f"update-{from_version}-to-{to_version}.zip"
    patch_name = f"{to_executable_path.name}-{from_version}-to-{to_version}.patch"
    files_to_package = []
    
    print(f"\n--- Creating package from '{from_version}' to '{to_version}' ---")

    # Compare manifests to find changed files
    for file, new_hash in to_manifest.items():
        if file not in from_manifest or from_manifest[file] != new_hash:
            files_to_package.append(file)
    
    print(f"Found {len(files_to_package)} new or modified files.")
    if not files_to_package:
        return None

    patch_info = None
    # If the main executable changed, create a patch for it
    if to_executable_path.name in files_to_package:
        if from_executable_path and from_executable_path.exists():
            print(f"Creating patch for {to_executable_path.name}...")
            bsdiff4.file_diff(from_executable_path, to_executable_path, patch_name)
            print(f"Patch created: {patch_name}")
            
            patch_info = {
                "file": to_executable_path.name,
                "patch_file": patch_name,
                "old_sha256": get_sha256(from_executable_path)
            }
            # Replace executable with its patch in the package list
            files_to_package.remove(to_executable_path.name)
            files_to_package.append(patch_name)
        else:
            print(f"Old executable for '{from_version}' not found. Including full executable.")

    # Create the update package .zip
    print(f"Creating update package '{package_name}'...")
    with ZipFile(package_name, "w") as zipf:
        # Create a mini-manifest specific to this package
        package_manifest = {"patch": patch_info} if patch_info else {}
        zipf.writestr("package-manifest.json", json.dumps(package_manifest, indent=4))

        # Add all other changed files
        for file in files_to_package:
            source_path = Path(file) if file == patch_name else build_dir / file
            zipf.write(source_path, arcname=file)
            
    package_size = os.path.getsize(package_name)
    print(f"Package '{package_name}' created successfully. Size: {package_size} bytes.")
    
    return {"file": package_name, "size": package_size, "from_version": from_version}


def main():
    parser = argparse.ArgumentParser(description="Create a differential update package with version history.")
    parser.add_argument("--build-dir", type=Path, required=True, help="Directory of the current build.")
    parser.add_argument("--new-version", type=str, required=True, help="The tag of the new version (e.g., v0.1.3).")
    parser.add_argument("--repo", type=str, required=True, help="The GitHub repository (e.g., 'user/repo').")
    parser.add_argument("--main-executable-name", type=str, default="main.exe", help="Name of the main executable file.")
    args = parser.parse_args()

    # --- 1. Initialization and Setup ---
    master_manifest_path = Path("manifest.json")
    new_version_tag = args.new_version
    
    # Load existing master manifest or create a new one
    manifest_fetched = False
    latest_tag = get_latest_tag(args.repo)
    if latest_tag and fetch_asset(args.repo, latest_tag, "manifest.json"):
        print(f"Loaded existing master manifest from release '{latest_tag}'.")
        with open(master_manifest_path, "r") as f:
            master_manifest = json.load(f)
        manifest_fetched = True

    if not manifest_fetched:
        print("No previous manifest found. Starting a new one.")
        master_manifest = {"versions": {}, "packages": {}}


    # Generate the file list for the new build
    print(f"Generating file list for new version '{new_version_tag}'...")
    new_version_files = generate_manifest_for_build(args.build_dir)
    master_manifest["versions"][new_version_tag] = new_version_files

    # --- 2. Identify Patch Targets ---
    sorted_versions = sorted(master_manifest["versions"].keys(), reverse=True)
    patch_targets = []
    
    # Target 1: Always create a patch from the immediate previous version
    previous_version = next((v for v in sorted_versions if v != new_version_tag), None)
    if previous_version:
        patch_targets.append({"version": previous_version, "type": "direct"})

    # Target 2: Create a cumulative "hop" patch every HOP_INTERVAL
    version_index = sorted_versions.index(new_version_tag)
    if len(sorted_versions) > HOP_INTERVAL and (version_index % HOP_INTERVAL == 0 or not previous_version):
         # Find the hop version, skipping intermediate ones
        hop_target_index = min(version_index + HOP_INTERVAL, len(sorted_versions) - 1)
        hop_version = sorted_versions[hop_target_index]
        if hop_version and hop_version not in [t["version"] for t in patch_targets]:
             patch_targets.append({"version": hop_version, "type": "cumulative"})

    if not patch_targets:
        print("No previous versions found to patch from. This is likely the first release.")
        master_manifest["packages"][new_version_tag] = []
    else:
        print(f"Identified patch targets: {[t['version'] for t in patch_targets]}")

    # --- 3. Generate Packages for Each Target ---
    new_packages = []
    for target in patch_targets:
        from_version = target["version"]
        
        # Download the executable for the target "from" version
        old_executable_name = f"main-executable-{from_version}.7z"
        if fetch_asset(args.repo, from_version, old_executable_name):
            subprocess.run(["7z", "x", old_executable_name, f"-oold-exec-{from_version}"], check=True)
            old_exe_path = Path(f"old-exec-{from_version}") / args.main_executable_name
        else:
            old_exe_path = None

        package_info = create_differential_package(
            from_version=from_version,
            to_version=new_version_tag,
            from_manifest=master_manifest["versions"][from_version],
            to_manifest=new_version_files,
            from_executable_path=old_exe_path,
            to_executable_path=args.build_dir / args.main_executable_name,
            build_dir=args.build_dir
        )
        if package_info:
            package_info["type"] = target["type"]
            new_packages.append(package_info)

    master_manifest["packages"][new_version_tag] = new_packages

    # --- 4. Finalize and Save ---
    with open(master_manifest_path, "w") as f:
        json.dump(master_manifest, f, indent=4)
    print("\nMaster manifest updated successfully.")
    
    # Set GitHub Actions output
    if 'GITHUB_OUTPUT' in os.environ and new_packages:
        with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
            print("package_created=true", file=f)
    elif 'GITHUB_OUTPUT' in os.environ:
         with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
            print("package_created=false", file=f)

if __name__ == "__main__":
    main()