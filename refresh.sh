#!/usr/bin/env bash
#
# Script to run in a weekly cron to update the website
#
# Updates the dump, re-builds and re-runs border-explorer, updates the gh-pages branch and pushes it
#
set -euo pipefail


SCRIPT_DIR=$(cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd)
function die() {
	echo "$@"
	exit 1
}
function cleanup() {
	if [ -f "$OLD_DUMP_FILE" ] ; then
		rm -f "$DUMP_FILE"
		mv "$OLD_DUMP_FILE" "$DUMP_FILE"
	fi
}
WORKDIR="$PWD"
DUMP_FILE=wikidata-dump.bz2
OLD_DUMP_FILE=wikidata-dump.old.bz2

if [ -f "$DUMP_FILE" ]; then
	mv "$DUMP_FILE" "$OLD_DUMP_FILE"
	trap cleanup EXIT
fi
# Is there a new available dump?
curl --retry 3 --silent --show-error --fail --time-cond "$OLD_DUMP_FILE" --continue-at - --location --remote-time --output "$DUMP_FILE" "https://dumps.wikimedia.org/wikidatawiki/entities/latest-all.json.bz2" || die "Failed to fetch new dump"

if [ -f "$DUMP_FILE" ]; then
	if [ -f "$OLD_DUMP_FILE" ]; then
		new_size=$(stat --format="%s" "$DUMP_FILE")
		old_size=$(stat --format="%s" "$OLD_DUMP_FILE")
		size_diff_percent=$(( (new_size - old_size) * 100 / old_size ))
		if [ ${size_diff_percent#-} -gt 5 ] ; then
			die "File difference higher than 5% ($size_diff_percent%): $new_size vs $old_size previously."
		fi
	fi
else
	echo "Nothing to do"
	exit 0
fi

# Successfully downloaded the dump, it's now the default
rm -f "$OLD_DUMP_FILE"
DAY_GENERATED=$(stat --format="%y" "$DUMP_FILE" |cut -d\  -f1)

# Build latest version of the tool
pushd "$SCRIPT_DIR"
git fetch --all
git pull --ff-only
cargo build --release
popd

# Remove any previously-generated data
rm -rf web/geojson border-explorer.db
# Process the dump
"$SCRIPT_DIR/target/release/border-explorer" border-explorer.db "$DUMP_FILE"

# Upload to github remote gh-pages branch
if [ ! -d gh-pages ]; then
	pushd "$SCRIPT_DIR"
	git worktree add -B gh-pages "$WORKDIR/gh-pages" github/gh-pages
	popd
fi

pushd gh-pages
git pull --ff-only
git rm geojson/*
mkdir -p geojson
cp ../web/geojson/* geojson/
sed -i -e "s, - Wikidata .*</a>, - Wikidata $DAY_GENERATED</a>," code.js
git add code.js geojson
git commit -m "Update to dump generated on $DAY_GENERATED"
git push github gh-pages

echo "Sucessfully updated repo with latest dump"
