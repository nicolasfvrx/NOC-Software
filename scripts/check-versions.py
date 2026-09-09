"""Reject releases whose tag, suite version or Cargo versions disagree."""
import os
from pathlib import Path
import tomllib

root = Path(__file__).resolve().parent.parent
version = (root / "VERSION").read_text().strip()
errors = []
if os.environ.get("GITHUB_REF_TYPE") == "tag":
    tag = os.environ.get("GITHUB_REF_NAME", "")
    if tag != f"v{version}":
        errors.append(f"Tag {tag!r} does not match VERSION {version}")
for app in ("noc-manager", "noc-display", "noc-agent"):
    for filename in ("Cargo.toml", "Cargo.lock"):
        data = tomllib.loads((root / app / filename).read_text())
        packages = data["package"]
        if isinstance(packages, dict):
            packages = [packages]
        package = next(p for p in packages if p["name"] == app)
        if package["version"] != version:
            errors.append(f"{app}/{filename}: {package['version']} != {version}")
if errors:
    raise SystemExit("\n".join(errors))
print(f"All Cargo manifests and lockfiles match NOC {version}.")
