# Border Explorer: viewing places that share border

Browse places that are connected by a border, connected as a graph.

This project contains a pipeline to extract from wikidata all the elements that share a border, and then organize them by categories to present them in a convenient view:

[Go to border explorer](https://anisse.github.io/border-explorer)


# Tech details

The [first version was viewing only French communes](https://anisse.astier.eu/wikidata-communes-viewer.html); I [rewrote the data pipeline in Rust](https://github.com/ansuz/RIIR) and made it generic to support any Wikidata category. On the website are the categories in the top 200, by number of edges; minus some categories that are too broad or generic.

Instead of learning SPARQL, I decided to do a first pass to put the data in an sqlite database; then a second pass generates geojson files to be viewed from SQL queries.

# Running, building, etc.

TODO.

# FAQ

### Why do some categories have such an non-descriptive name?

It comes from wikidata. Usually those categories have a good enough full description, but a very short name, like "district" or "province"; do not hesitate to contribute to wikidata to improve those in your language!

### Why do I only see names in English (or French)?

Sometimes there might not even be a name for any locale, so for english we fallback to `mul`, the multilingual label. But in general, I have only implemented getting the names for those two locales, I might do more if there is interest!

### Why did you do this?

Good question. If you find out, let me know!

### I am not happy with the names displayed, or the political borders described here

I invite you to open a discussion about this on Wikidata's wiki Discussion pages.

### When will you update to a more recent Wikidata dump?

Soon™…


