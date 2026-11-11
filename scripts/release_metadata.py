"""Validate a release tag against Cargo.toml and extract its changelog notes."""

import argparse
import json
import os
from pathlib import Path
import re
import tomllib


def release_metadata(root: Path, tag: str) -> tuple[str, str]:
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    match = re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)(?:\.(0|[1-9]\d*))?(-[0-9A-Za-z.-]+)?", tag)
    if match is None:
        raise ValueError("Release tag must be a version such as v3.0 or v3.0.0, not a branch name")
    normalized = f"{match[1]}.{match[2]}.{match[3] or '0'}{match[4] or ''}"
    if normalized != version:
        raise ValueError(f"Tag {tag} does not match Cargo package version {version}")
    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    section = re.search(rf"^## {re.escape(version)}(?:[ \t][^\n]*)?\n(.*?)(?=^## |\Z)", changelog, re.MULTILINE | re.DOTALL)
    if section is None or not section[1].strip():
        raise ValueError(f"CHANGELOG.md has no release notes for {version}")
    return version, section[1].strip() + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes-file", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        version, notes = release_metadata(root, args.tag)
    except (ValueError, KeyError, OSError) as error:
        parser.error(str(error))
    if args.notes_file is not None:
        args.notes_file.write_text(notes, encoding="utf-8")
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"tag={args.tag}\nversion={version}\n")
    print(json.dumps({"tag": args.tag, "version": version}))


if __name__ == "__main__":
    main()
