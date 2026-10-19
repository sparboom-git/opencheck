#!/usr/bin/env sh
# Downloads the buildingSMART certification sample models used for local testing.
# Source: https://github.com/buildingSMART/Sample-Test-Files (Certification-datasets)
set -e
DEST="$(dirname "$0")/../testdata"
BASE="https://raw.githubusercontent.com/buildingSMART/Sample-Test-Files/main"
mkdir -p "$DEST/ifc4" "$DEST/ifc2x3"
for f in Building-Architecture Building-Structural Building-Hvac Building-Landscaping; do
  curl -fsSL "$BASE/IFC%204.0.2.1%20(IFC%204%20ADD2%20TC1)/Simple-Scene/$f.ifc" -o "$DEST/ifc4/$f.ifc"
  curl -fsSL "$BASE/IFC%202.3.0.1%20(IFC%202x3%20TC1)/Simple-Scene/$f.ifc" -o "$DEST/ifc2x3/$f.ifc"
done
curl -fsSL "$BASE/IFC%204.0.2.1%20(IFC%204%20ADD2%20TC1)/ISO%20Spec%20-%20ReferenceView_V1.2/wall-with-opening-and-window.ifc" -o "$DEST/ifc4/wall-with-opening-and-window.ifc"
echo "Test data written to $DEST"
