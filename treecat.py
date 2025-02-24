#!/usr/bin/env python3

import os

################################################################################
# You can add/remove items here to exclude them by exact name or pattern. 
################################################################################
EXCLUDES = {
    ".git",       # entire .git folder
    "target", 
    "build", 
    "Cargo.lock",
    ".gitignore",
    ".DS_Store",
    # Add anything else you consider "dumb shit" to skip
}

def should_exclude(path_name: str) -> bool:
    """
    Decide whether to exclude a file or directory based on its name.
    Currently, we exclude anything in the EXCLUDES set by exact name.
    """
    return path_name in EXCLUDES

def print_tree_and_contents(path, prefix=""):
    """
    Recursively print the directory tree of `path`.
    After printing each filename, print its contents inline, indented.
    """

    try:
        entries = os.listdir(path)
    except OSError:
        return

    # Filter out directories/files we want to ignore
    # (by checking if `entry` is in EXCLUDES or if there's any other rule you want)
    entries = [e for e in entries if not should_exclude(e)]

    # Sort so that directories come first, then files (alphabetically)
    def sort_key(name):
        full = os.path.join(path, name)
        return (not os.path.isdir(full), name.lower())

    entries.sort(key=sort_key)

    for i, entry in enumerate(entries):
        full_path = os.path.join(path, entry)
        is_last = (i == len(entries) - 1)

        branch = "└── " if is_last else "├── "
        print(prefix + branch + entry)

        if os.path.isdir(full_path):
            # Recurse into subdirectories
            extension = "    " if is_last else "│   "
            print_tree_and_contents(full_path, prefix + extension)
        else:
            # It's a file; print its contents indented
            file_prefix = prefix + ("    " if is_last else "│   ")
            try:
                with open(full_path, "r", encoding="utf-8") as f:
                    for line in f:
                        print(file_prefix + line.rstrip())
            except Exception as e:
                # If it's binary or unreadable, show an error
                print(file_prefix + f"[Could not read file: {e}]")

if __name__ == "__main__":
    root_directory = "."  # or pass as sys.argv[1] if you want a custom path
    print_tree_and_contents(root_directory)
