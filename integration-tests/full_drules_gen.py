#!/usr/bin/env python3

import sys
from copy import deepcopy
from optparse import OptionParser
from pathlib import Path
import logging

# Add `src` directory to the import paths
sys.path.insert(0, str(Path(__file__).parent.parent / 'src'))

import libkomwm

FORMAT = '%(asctime)s [%(levelname)s] %(message)s'
logging.basicConfig(format=FORMAT)
log = logging.getLogger('test_drules_gen')
log.setLevel(logging.INFO)

styles = {
    'drules_proto_default_light':  ['styles/default/light/style.mapcss',  'styles/default/include'],
    'drules_proto_default_dark':   ['styles/default/dark/style.mapcss',   'styles/default/include'],
    'drules_proto_outdoors_light': ['styles/outdoors/light/style.mapcss', 'styles/outdoors/include'],
    'drules_proto_outdoors_dark':  ['styles/outdoors/dark/style.mapcss',  'styles/outdoors/include'],
    'drules_proto_vehicle_light':  ['styles/vehicle/light/style.mapcss',  'styles/vehicle/include'],
    'drules_proto_vehicle_dark':   ['styles/vehicle/dark/style.mapcss',   'styles/vehicle/include'],
}


def full_styles_regenerate(options):
    log.info("Start generating styles")
    libkomwm.MULTIPROCESSING = False
    prio_ranges_orig = deepcopy(libkomwm.prio_ranges)

    for name, (style_path, include_path) in styles.items():
        log.info(f"Generating {name} style ...")

        # Restore initial state
        libkomwm.prio_ranges = deepcopy(prio_ranges_orig)
        libkomwm.visibilities = {}

        options.filename = options.data + '/' + style_path
        options.priorities_path = options.data + '/' + include_path
        options.outfile = options.outdir + '/' + name

        # Run generation
        libkomwm.komap_mapswithme(options)

def compare_with_main_repo(generated_dir, data_dir):
    has_any_diff = False
    for style_name in styles:
        match = compare_content(f"{generated_dir}/{style_name}.bin", f"{data_dir}/{style_name}.bin", binary=True)
        if not match:
            log.warning(f"File {style_name}.bin doesn't match")
            has_any_diff = True
        match = compare_content(f"{generated_dir}/{style_name}.txt", f"{data_dir}/{style_name}.txt", binary=False)
        if not match:
            log.warning(f"File {style_name}.txt doesn't match")
            has_any_diff = True

    if not has_any_diff:
        log.info("All generated files match")

def compare_content(file_a_path: str, file_b_path:str, binary=True) -> bool:
    mode = "rb" if binary else "rt"
    with open(file_a_path, mode) as file_a_obj, open(file_b_path, mode) as file_b_obj:
        file_a_content = file_a_obj.read()
        file_b_content = file_b_obj.read()
        return file_a_content == file_b_content

def main():
    parser = OptionParser()
    parser.add_option("-d", "--data-path", dest="data",
                      help="path to mapcss-mapping.csv and other files", metavar="PATH")
    parser.add_option("-o", "--output-dir", dest="outdir", default="drules",
                      help="output directory", metavar="DIR")
    parser.add_option("-f", "--minzoom", dest="minzoom", default=0, type="int",
                      help="minimal available zoom level", metavar="ZOOM")
    parser.add_option("-t", "--maxzoom", dest="maxzoom", default=20, type="int",
                      help="maximal available zoom level", metavar="ZOOM")
    parser.add_option("-x", "--txt", dest="txt", action="store_true",
                      help="create a text file for output", default=False)

    (options, args) = parser.parse_args()

    if options.data is None:
        parser.error("Please specify base 'data' path.")

    if options.outdir is None:
        parser.error("Please specify base output path.")

    full_styles_regenerate(options)
    compare_with_main_repo(options.outdir, options.data)
    log.info("Done!")

if __name__ == '__main__':
    main()
