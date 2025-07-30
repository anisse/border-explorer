
const params = {};
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
}
const map = new maplibregl.Map({
	container: 'map',
	style: {version: 8,sources: {},layers: [], glyphs: "{fontstack}/{range}.pbf" },
	attributionControl: {customAttribution: "<a href='https://anisse.astier.eu/wikidata-communes-viewer.html' target='_blank'>Anisse Astier</a>", compact: true},
	center: params.center || [15,15],
	zoom: params["zoom"] || 1.6
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
async function getNodes(id) {
	places = await getData("geojson/" + id + "-nodes.geojson")
}
async function getLinks(id) {
	links = await getData("geojson/" + id + "-links.geojson")
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
				// zoom is 10 (or greater) -> circle radius will be 5px
				10, 5
			]
		}
	});
	map.addLayer({
		id: 'labels',
		type: 'symbol',
		source: 'places',
		layout: {
			'text-field': ['get', 'en'],
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
		if (!value)
			return;
		const filter = ['in', value.trim().toLowerCase(), ['downcase', ['get', 'en']]];
		map.setFilter('places', filter);
		map.setFilter('labels', filter);
	}
	filterInput.addEventListener('keyup', (e) => {
		//const value = e.target.value.trim().toLowerCase();
		updateFilter(e.target.value);
	});
	updateFilter(params.filter);
}
async function initSelection() {
	const select = document.getElementById("category");
	Object.entries(index)
		.sort(([,a],[,b]) => a.en.toLowerCase() > b.en.toLowerCase())
		.forEach(([key, value]) => {
			var option = document.createElement("option");
			option.text = value.en;
			option.value = key;
			select.add(option);
		})
}
async function onSelect() {
	const select = document.getElementById("category");
	select.addEventListener("change", (event) => {
		const id = event.target.value;
		getLinks(id).then(() => {
			map.getSource("places_links").setData(links.data);
		});
		getNodes(id).then(() => {
			map.getSource("places").setData(places.data);
		});
	});
}
Promise.all(
	[
	Promise.all([
		load.then(process),
		getBgLayer(),
		]).then(loadBgLayer),
	getIndex().then(initSelection),
	]).then(onSelect);
