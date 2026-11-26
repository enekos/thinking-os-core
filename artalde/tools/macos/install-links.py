#!/usr/bin/env python3
"""Register artalde:// links. --web also requests routing of ordinary web links."""
import argparse
import os
import platform
import plistlib
import shutil
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--web", action="store_true")
args = parser.parse_args()
target = Path.home() / "Applications" / "Artalde Links.app"
executable = shutil.which("artalde")
if not executable:
    parser.error("install artalde on PATH first")
identifier = "org.thinking-os.artalde.links"
prior = {}
if target.exists():
    prior = plistlib.loads((target / "Contents/Info.plist").read_bytes())
    if prior.get("CFBundleIdentifier") != identifier:
        parser.error(f"{target} belongs to another application")
browser_lookup = 'import AppKit; if let app = NSWorkspace.shared.urlForApplication(toOpen: URL(string:"https://example.com")!), let id = Bundle(url:app)?.bundleIdentifier { print(id) }'
fallback = subprocess.check_output(["swift", "-e", browser_lookup], text=True).strip()
if fallback == identifier:
    fallback = prior.get("ArtaldeFallbackBrowser", "")
if not fallback:
    parser.error("select a web browser before installing the URL handler")
info = {
    "CFBundleName": "Artalde Links", "CFBundleIdentifier": identifier,
    "CFBundleVersion": "1", "CFBundlePackageType": "APPL", "LSUIElement": True,
    "LSMinimumSystemVersion": "12.0", "CFBundleExecutable": "ArtaldeLinks", "ArtaldeExecutable": os.path.abspath(executable),
    "ArtaldeFallbackBrowser": fallback,
    "ArtaldeToolPaths": list(dict.fromkeys(str(Path(p).parent) for name in ("artalde", "gh", "git") if (p := shutil.which(name)))),
    "CFBundleURLTypes": [{"CFBundleURLName": "Artalde pull requests", "CFBundleTypeRole": "Viewer", "CFBundleURLSchemes": ["artalde", "https", "http"]}],
}
target.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(dir=target.parent) as scratch:
    bundle = Path(scratch) / target.name
    macos = bundle / "Contents/MacOS"
    macos.mkdir(parents=True)
    (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
    subprocess.run(["swiftc", "-O", "-target", f"{platform.machine()}-apple-macosx12.0", str(Path(__file__).with_name("ArtaldeLinks.swift")), "-o", str(macos / "ArtaldeLinks")], check=True)
    subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], check=True)
    backup = Path(scratch) / "previous.app"
    if target.exists():
        target.rename(backup)
    try:
        bundle.rename(target)
    except OSError:
        if backup.exists():
            backup.rename(target)
        raise
register = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
subprocess.run([register, "-f", str(target)], check=True)
print(f"Registered artalde:// with {target}", flush=True)
if args.web:
    select = """
import AppKit
let app = URL(fileURLWithPath: CommandLine.arguments[1])
Task {
    do {
        for scheme in ["https", "http"] {
            try await NSWorkspace.shared.setDefaultApplication(at: app, toOpenURLsWithScheme: scheme)
        }
        exit(0)
    } catch {
        FileHandle.standardError.write(Data(error.localizedDescription.utf8))
        exit(1)
    }
}
RunLoop.main.run()
"""
    subprocess.run(["swift", "-e", select, str(target)], check=True)
