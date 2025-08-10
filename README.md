# Border Explorer: viewing places that share border

Browse places that are connected by a border, connected as a graph.

This project contains a pipeline to extract from Wikidata all the elements that share a border, and then organize them by categories to present them in a convenient view:

[Go to border explorer](https://anisse.github.io/border-explorer)


# Tech details

The [first version was viewing only French communes](https://anisse.astier.eu/wikidata-communes-viewer.html); I [rewrote the data pipeline in Rust](https://github.com/ansuz/RIIR) and made it generic to support any Wikidata category. On the website are the categories in the top 200, by number of edges; minus some categories that are too broad or generic.

Instead of learning SPARQL, I decided to do a first pass to put the data in an sqlite database; then a second pass generates geojson files to be viewed from SQL queries.

# Dependencies

 - lbzip2's lbzcat
 - a recent rust toolchain
 - a C compiler (to re-build sqlite)

Crates:

 - chrono: for parsing times in the dumps
 - rusqlite: for sqlite access - uses a bundled version of sqlite3
 - memchr: for fast filtering before json parsing
 - serde and serde_json: for JSON parsing and geojson file generation
 - reqwest: for fetching category names from Wikidata (< 200 HTTP requests in a run)
 - indexmap: for stable output generation

# Running, building, etc.

Build and run on a [bz2 JSON dump of Wikidata](https://www.wikidata.org/wiki/Wikidata:Database_download#JSON_dumps_(recommended)), storing the extracted information in temporary sqlite database `border-explorer.db`:

```sh
cargo run --release border-explorer.db ./wikidata/latest-all.json.bz2
```

This will generate the geojson files in `web/geojson/`; you can then use the website statically with a webserver at the root of `web/`.

# FAQ

### Why do some categories have such an non-descriptive name?

It comes from Wikidata. Usually those categories have a good enough full description, but a very short name, like "district" or "province"; do not hesitate to contribute to Wikidata to improve those in your language!

### Why do some categories seem to have incomplete information?

It comes from Wikidata. Do not hesitate to [contribute](https://www.wikidata.org/wiki/Wikidata:Contribute)!

### Why do I only see names in English (or French)?

Sometimes there might not even be a name for any locale, so for english we fallback to `mul`, the multilingual label. But in general, I have only implemented getting the names for those two locales, I might do more if there is interest!

### Why did you do this?

Good question. If you find out, let me know!

### I am not happy with the names displayed, or the political borders described here

I invite you to open a discussion about this on Wikidata's wiki Discussion pages.

### When will you update to a more recent Wikidata dump?

The current version comes from a July 30th Wikidata dump (size: 97523272340 bytes, sha256: 022ebd511a6a61d39088fbbb6e3e16417562e415556b5f01462260d573c3af9e). In the future, I'd like to setup a weekly cron to get the dumps and run it automatically to get regular updates.


