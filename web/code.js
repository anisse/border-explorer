
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
	layer = await getData("earth-coastlines-10km.geo.json")
        map.addSource('background', {
		type: 'geojson',
		data: layer
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

async function getIndex() {
	index = await getData("geojson/index.json")
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
	map.addSource('places_links', links);
	map.addLayer({
		'id': 'links',
		'type': 'line',
		'minzoom': 7,
		'source': 'places_links',
		'paint': {
			'line-color': '#cdcdcd'
		}
	});
	map.addSource('places', places);
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
			'text-field': ['get', 'fr'],
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
		const filter = ['in', value.trim().toLowerCase(), ['downcase', ['get', 'fr']]];
		map.setFilter('places', filter);
		map.setFilter('labels', filter);
	}
	filterInput.addEventListener('keyup', (e) => {
		//const value = e.target.value.trim().toLowerCase();
		updateFilter(e.target.value);
	});
	updateFilter(params.filter);
}
Promise.all(
	[
	load.then(loadBgLayer),
	getIndex(),
	getNodes("Q484170"),
	getLinks("Q484170"),
	]).then(process);
