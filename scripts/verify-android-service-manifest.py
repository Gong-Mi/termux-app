#!/usr/bin/env python3
"""Verify Android 14/15 special-use foreground-service declarations."""
from pathlib import Path
import sys
import xml.etree.ElementTree as ET

ANDROID = "http://schemas.android.com/apk/res/android"
A = lambda name: f"{{{ANDROID}}}{name}"

manifest_path = Path(__file__).resolve().parents[1] / "app/src/main/AndroidManifest.xml"
root = ET.parse(manifest_path).getroot()

special_use_permission = "android.permission.FOREGROUND_SERVICE_SPECIAL_USE"
permissions = [node.get(A("name")) for node in root.findall("uses-permission")]
if permissions.count(special_use_permission) != 1:
    raise SystemExit(
        f"expected exactly one {special_use_permission}, "
        f"found {permissions.count(special_use_permission)}"
    )

services = {
    node.get(A("name")): node
    for node in root.find("application").findall("service")
}
expected = {
    ".app.TermuxService",
    ".app.RunCommandService",
}
missing = expected - services.keys()
if missing:
    raise SystemExit(f"missing service declarations: {sorted(missing)}")

property_name = "android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE"
for name in sorted(expected):
    service = services[name]
    if service.get(A("foregroundServiceType")) != "specialUse":
        raise SystemExit(f"{name} does not declare foregroundServiceType=specialUse")
    properties = [
        node for node in service.findall("property") if node.get(A("name")) == property_name
    ]
    if len(properties) != 1 or not (properties[0].get(A("value")) or "").strip():
        raise SystemExit(f"{name} must have one non-empty {property_name} property")

print(f"PASS Android special-use foreground-service manifest: {manifest_path}")
