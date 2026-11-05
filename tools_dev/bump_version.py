"""Set Aura Portfolio's version everywhere it's written, in one go:

    python tools_dev/bump_version.py 1.2.0

VERSION (the single source of truth), Cargo.toml and Cargo.lock (build.rs refuses to build when they differ from
VERSION, and so does CI), and a new `## [X.Y.Z] - <date>` section at the top of CHANGELOG.md to fill in. Then build
once (`cargo clippy --locked ...`), commit and push to main: the Release workflow sees VERSION change, builds the
programs, tags vX.Y.Z and publishes them.

Pick the number from the newest tag (`git fetch --tags; git tag --sort=-v:refname`), not only from VERSION:
patch = fixes, minor = features, major = breaking changes to data, config or the install layout.
"""
import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = "aura-portfolio"


def main(new: str) -> None:
    if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", new):
        sys.exit(f"{new!r} isn't a version like 1.2.0")

    (ROOT / "VERSION").write_text(new + "\n", encoding="utf-8", newline="\n")

    cargo = ROOT / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    text, n = re.subn(r'(?m)^version = "[^"]*"', f'version = "{new}"', text, count=1)
    if n != 1:
        sys.exit("Cargo.toml: no package version line found")
    cargo.write_text(text, encoding="utf-8", newline="\n")

    lock = ROOT / "Cargo.lock"
    if lock.exists():
        text = lock.read_text(encoding="utf-8")
        text, n = re.subn(rf'(name = "{CRATE}"\r?\nversion = )"[^"]*"', rf'\1"{new}"', text)
        if n != 1:
            sys.exit(f"Cargo.lock: no {CRATE} entry found (run cargo check once)")
        lock.write_text(text, encoding="utf-8", newline="\n")

    log = ROOT / "CHANGELOG.md"
    text = log.read_text(encoding="utf-8") if log.exists() else "# Changelog\n"
    if f"## [{new}]" not in text:
        head, sep, rest = text.partition("\n## ")
        entry = f"## [{new}] - {datetime.date.today().isoformat()}\n\n- \n"
        text = f"{head.rstrip()}\n\n{entry}" + (f"\n## {rest}" if sep else "")
        log.write_text(text, encoding="utf-8", newline="\n")

    print(f"Version {new}: VERSION, Cargo.toml, Cargo.lock and a CHANGELOG.md section.\n"
          f"Next: fill in CHANGELOG.md, run `cargo clippy --locked --all-targets -- -D warnings` and "
          f"`cargo test --locked`, commit, and push to main - v{new} is then built and published by itself.")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
