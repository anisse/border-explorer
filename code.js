
const params = {};
var blockNextFit;
if (window.location.hash) {
	window.location.hash.substring(1).split('&').forEach(item => {
		const key = item.split('=')[0];
		let val = decodeURIComponent(item.split('=')[1]);
		if (key == "zoom")
			val = parseFloat(val);
		if (key == "center")
			val = val.split(',').map(item => parseFloat(item));
		params[key] = val;
	});
	if (params.category && (params.zoom != undefined || params.center != undefined)) {
		/* Prevent automatic loading of the category to go to different coordinates */
		blockNextFit = true;
	}
}
function getUrl() {
	// Limit to 5 decimal digits to simplify URLs (precision: 1.11m)
	const formatNum = (num) => num.toPrecision(5).replace(/0*$/, '');
	var url = window.location.protocol + "//" + window.location.host+
		window.location.pathname + "#zoom=" + formatNum(map.getZoom()) +
		"&center=" + formatNum(map.getCenter().lng) + "," + formatNum(map.getCenter().lat);
	const category = document.getElementById("category").value;
	if (category)
		url += "&category=" + category;
	const filter = document.getElementById("filter-input").value;
	if (filter)
		url += "&filter=" + filter;
	return url;
}
function setUrl() {
	window.location.replace(getUrl());
}
const map = new maplibregl.Map({
	container: 'map',
	style: {version: 8,sources: {},layers: [], glyphs: "{fontstack}/{range}.pbf" },
	attributionControl: {customAttribution: "<a href='https://github.com/anisse/border-explorer' target='_blank'>Border Explorer by Anisse Astier</a>", compact: true},
	center: params.center || [0,0],
	zoom: params["zoom"] || 1.5
});
map.addControl(new maplibregl.NavigationControl({
	visualizePitch: false,
	visualizeRoll: false,
	showZoom: true,
	showCompass: false
}), 'bottom-right');
const filterInput = document.getElementById('filter-input');
filterInput.value = params.filter || "";


// disable map rotation using right click + drag
map.dragRotate.disable();
// disable map rotation using keyboard
map.keyboard.disable();
// disable map rotation using touch rotation gesture
map.touchZoomRotate.disableRotation();
// disable map pitch using touch gesture
map.touchPitch.disable();

const load = map.once('load')

async function loadBgLayer() {
        map.addSource('background', {
		type: 'geojson',
		data: bgLayer
	});
	map.addLayer({
            'id': 'background',
            'type': 'line',
            'source': 'background',
            'layout': {
                'line-join': 'round',
                'line-cap': 'round'
            },
            'paint': {
                'line-color': '#888',
                'line-width': 2
            }
        });
}


var index;
var places;
var links;
var bgLayer;

async function getIndex() {
	index = await getData("geojson/index.json")
}
async function getBgLayer() {
	bgLayer = await getData("earth-coastlines-10km.geo.json")
}
async function getData(url) {
	try {
		const response = await fetch(url);
		if (!response.ok) {
			throw new Error(`Response status: ${response.status}`);
		}

		const json = await response.json();
		return json;
	} catch (error) {
		console.error(error.message);
	}
}
async function process() {
	map.addSource('places_links', {
		'type': 'geojson',
		'data': {
			'type': 'MultiLineString',
			'coordinates': [],
		},
	});
	map.addLayer({
		'id': 'links',
		'type': 'line',
		'minzoom': 2,
		'source': 'places_links',
		'paint': {
			'line-color': '#cdcdcd'
		}
	});
	map.addSource('places', {
		'type': 'geojson',
		'data': {
			'type': 'FeatureCollection',
			'features': [],
		},
	});
	map.addLayer({
		'id': 'places',
		'type': 'circle',
		'source': 'places',
		'paint': {
			'circle-color': '#5470c6',
			"circle-radius": [
				"interpolate", ["linear"], ["zoom"],
				// zoom is 5 (or less) -> circle radius will be 1px
				5, 1,
				10, 6,
				// zoom is 15 (or greater) -> circle radius will be 9px
				15, 9
			]
		}
	});
	const getLabelExpr = ['case', ['to-boolean', ['get', detectedLanguage]], ['get', detectedLanguage], ['get', 'en']];
	map.addLayer({
		id: 'labels',
		type: 'symbol',
		source: 'places',
		layout: {
			'text-field': getLabelExpr,
			'text-variable-anchor': ['top', 'bottom', 'left', 'right'],
			'text-radial-offset': 0.5,
			'text-justify': 'auto',
			'text-font': ["Noto Sans Regular"],
			'text-padding': 4,
			'text-optional': true,
		},
		paint: {
			'text-halo-color': '#ffffff',
			'text-halo-width': 1,
		}
	}
	);
	function updateFilter(value) {
		if (!value) {
			map.setFilter('places', null);
			map.setFilter('labels', null);
			return;
		}
		const filter = ['in', value.trim().toLowerCase(), ['downcase', getLabelExpr]];
		map.setFilter('places', filter);
		map.setFilter('labels', filter);
	}
	filterInput.addEventListener('keyup', (e) => {
		//const value = e.target.value.trim().toLowerCase();
		updateFilter(e.target.value);
	});
	updateFilter(params.filter);
	map.on("sourcedata", (e) => {
		if (e.sourceId != 'places' || !e.isSourceLoaded || e.sourceDataType != "metadata")
			return;
		if (blockNextFit) {
			blockNextFit = false
			return;
		}
		map.getSource('places').getBounds().then(bounds => map.fitBounds(bounds));
	});
	map.on("idle", setUrl);
}
async function initSelection() {
	const select = document.getElementById("category");
	Object.entries(index)
		.map(([key, value]) => [key, value[detectedLanguage] || value["en"]])
		.sort(([,a],[,b]) => a.toLowerCase().localeCompare(b.toLowerCase()))
		.forEach(([key, text]) => {
			var option = document.createElement("option");
			option.text = text;
			option.value = key;
			select.add(option);
		})
	const random = document.getElementById("randomBtn");
	random.addEventListener("click", (event) => {
		let num = Math.floor(Math.random() * (select.options.length -1));
		select.selectedIndex = num + 1; // skip element 0
		select.dispatchEvent(new Event("change"));
	});
}
function getLanguage() {
	const langList = navigator.languages || ["en"];
	return langList
		.map((l) => l.split("-")[0])
		.filter((l) => ["en", "fr"].includes(l))
		[0] || "en";
}
const detectedLanguage = getLanguage();
async function onSelect() {
	const select = document.getElementById("category");
	select.addEventListener("change", (event) => {
		const id = event.target.value;
		map.getSource("places_links").setData("geojson/" + id + "-links.geojson");
		map.getSource("places").setData("geojson/" + id + "-nodes.geojson");
	});
	select.selectedIndex = 0;
	if (params.category in index) {
		select.value = params.category;
		select.dispatchEvent(new Event("change"));
	}
}
Promise.all(
	[
	Promise.all([
		load.then(process),
		getBgLayer(),
		]).then(loadBgLayer),
	getIndex().then(initSelection),
	]).then(onSelect);
