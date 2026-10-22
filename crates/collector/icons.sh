#!/bin/sh
# Builds ui/icons.svg, the page's icon sprite, from Lucide (ISC licence).
# Add an icon: put its name from lucide.dev in ICONS and run `make icons`.
# On the page: <svg class="icon"><use href="icons.svg#name"/></svg>; the
# stroke comes from .icon in style.css.
set -eu

VERSION=1.52.0
ICONS="arrow-left chevron-down crosshair menu pause pencil-ruler play radio rotate-ccw rotate-cw scan x zoom-in zoom-out"

out="$(dirname "$0")/ui/icons.svg"
{
	echo "<svg xmlns=\"http://www.w3.org/2000/svg\">"
	echo "<!-- Lucide v$VERSION (ISC licence, lucide.dev). Built by icons.sh: edit that, not this. -->"
	for name in $ICONS; do
		# the drawing between <svg ...> and </svg>
		body=$(curl -fsS "https://unpkg.com/lucide-static@$VERSION/icons/$name.svg" |
			sed -n '/^>$/,/^<\/svg>$/p' | sed '1d;$d' | sed 's/^ *//' | tr -d '\n')
		[ -n "$body" ] || { echo "icons.sh: $name: nothing drawn" >&2; exit 1; }
		echo "<symbol id=\"$name\" viewBox=\"0 0 24 24\">$body</symbol>"
	done
	echo "</svg>"
} > "$out.tmp"
mv "$out.tmp" "$out"
echo "$out: $(echo $ICONS | wc -w | tr -d ' ') icons"
