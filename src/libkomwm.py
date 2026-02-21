import json
import logging
from typing import List, Dict, Set, Optional, Callable, NamedTuple, Union
from mapcss import MapCSS
from optparse import OptionParser, Values
import os
from sys import exit
from multiprocessing import Pool, set_start_method

from drules_config import get_prio_ranges, LAYER_PRIORITY_RANGE
from drules_utils import mwm_encode_color
from drules_priority import PriorityManager
from drules_classificator import ClassificatorManager, load_mapcss_dynamic_tags
from drules_fileio import FileIOManager
from drules_visibility import VisibilityTracker
from drules_style_processor import StyleProcessor
from drules_serializer import DrulesSerializer

# Configure module logger
logger = logging.getLogger(__name__)

# =============================================================================
# CONSTANTS
# =============================================================================

# Required tag names for all classifications
REQUIRED_TAGS = {
    'name': 'name',
    'addr:housenumber': 'addr:housenumber',
    'addr:housename': 'addr:housename',
    'ref': 'ref',
    'int_name': 'int_name',
    'addr:flats': 'addr:flats'
}

# Visibility string format prefix
VISIBILITY_PREFIX = 'world|'
VISIBILITY_SUFFIX = '|'

# =============================================================================
# TYPE ALIASES
# =============================================================================

# Recursive type for nested dictionary values
DictValue = Union[str, int, float, bool, None, 'Dict[str, DictValue]', 'List[DictValue]']

# Type for drules element dictionaries (more specific)
DrawElementDict = Dict[str, Union[int, str, List, Dict, None]]

# Type for classification dictionaries
ClassificationDict = Dict[str, Union[str, List[DrawElementDict]]]

# Type for top-level drules data
DrulesDataDict = Dict[str, Union[List, List[ClassificationDict]]]


# =============================================================================
# DATA CLASSES FOR TYPE SAFETY
# =============================================================================

class StyleQueryArgs(NamedTuple):
    """Arguments for querying style for a classification type."""
    cl: str  # Classification name
    cltags: Dict[str, str]  # Classification tags
    minzoom: int  # Minimum zoom level
    maxzoom: int  # Maximum zoom level


class StyleQueryResult(NamedTuple):
    """Result from querying style for a classification type."""
    cl: str  # Classification name
    zoom: int  # Zoom level
    runtime_conditions: Optional[List]  # Runtime conditions (MapCSS runtime objects)
    zstyle: List[Dict[str, str]]  # Style list


class StyleAnalysis(NamedTuple):
    """Result of analyzing style types."""
    has_lines: bool
    has_icons: bool
    has_fills: bool
    has_text: Optional[List[Dict[str, str]]]


# =============================================================================
# STYLE PROCESSING FUNCTIONS
# =============================================================================

def query_style(args: StyleQueryArgs) -> List[StyleQueryResult]:
    """
    Query styles for a classification type across zoom levels.

    Note: Uses global 'style' variable for multiprocessing efficiency.
    Passing large MapCSS objects to workers causes significant serialization overhead.

    Args:
        args: StyleQueryArgs containing classification info and zoom range

    Returns:
        List of StyleQueryResult objects
    """
    global style
    cl, cltags, minzoom, maxzoom = args

    # Extract classification name (before first '-')
    dash_pos = cl.find('-')
    clname = cl if dash_pos == -1 else cl[:dash_pos]

    # Add required tags (used for label rendering)
    cltags.update(REQUIRED_TAGS)

    results = []
    for zoom in range(minzoom, maxzoom + 1):
        all_runtime_conditions_arr = []

        # Get runtime conditions
        if "area" not in cltags:
            all_runtime_conditions_arr.extend(style.get_runtime_rules(clname, "line", cltags, zoom))
        all_runtime_conditions_arr.extend(style.get_runtime_rules(clname, "area", cltags, zoom))
        if "area" not in cltags:
            all_runtime_conditions_arr.extend(style.get_runtime_rules(clname, "node", cltags, zoom))

        # Filter unique runtime conditions (use hash-based deduplication)
        runtime_conditions_arr = []
        if len(all_runtime_conditions_arr) == 0:
            runtime_conditions_arr.append(None)
        elif len(all_runtime_conditions_arr) == 1:
            runtime_conditions_arr = all_runtime_conditions_arr
        else:
            # Use string representation for deduplication (conditions aren't hashable)
            seen = set()
            for rt_conditions in all_runtime_conditions_arr:
                key = str(rt_conditions)
                if key not in seen:
                    seen.add(key)
                    runtime_conditions_arr.append(rt_conditions)

        for runtime_conditions in runtime_conditions_arr:
            # Reuse dictionary instead of creating new ones for better performance
            zstyle = {}

            # Get and merge styles (updates zstyle in-place)
            if "area" not in cltags:
                style.get_style_dict(clname, "line", cltags, zoom, olddict=zstyle,
                                     filter_by_runtime_conditions=runtime_conditions)
            style.get_style_dict(clname, "area", cltags, zoom, olddict=zstyle,
                                 filter_by_runtime_conditions=runtime_conditions)
            if "area" not in cltags:
                style.get_style_dict(clname, "node", cltags, zoom, olddict=zstyle,
                                     filter_by_runtime_conditions=runtime_conditions)

            results.append(StyleQueryResult(
                cl=cl,
                zoom=zoom,
                runtime_conditions=runtime_conditions,
                zstyle=list(zstyle.values())
            ))

    return results


def _extract_style_colors(style: MapCSS, colors: Set[int]) -> Dict[str, int]:
    """Extract and encode colors from style."""
    style_colors = {}
    raw_style_colors = style.get_colors()
    if raw_style_colors is not None:
        unique_style_colors = set()
        for k in list(raw_style_colors.keys()):
            unique_style_colors.add(k[:k.rindex('-')])
        for k in unique_style_colors:
            style_colors[k] = mwm_encode_color(colors, raw_style_colors, k)
    return style_colors


def _add_colors_to_drules(drules_data: DrulesDataDict, style_colors: Dict[str, int]) -> None:
    """Add color definitions to drules data dictionary."""
    if style_colors:
        drules_data['colors'] = []
        for k, v in sorted(list(style_colors.items())):
            drules_data['colors'].append({
                'name': k,
                'color': v,
                'x': 0,
                'y': 0
            })


def _analyze_style_types(zstyle: List[Dict[str, str]]) -> StyleAnalysis:
    """
    Analyze what types of styles are present (lines, fills, icons, text).

    Returns:
        StyleAnalysis with flags indicating which style types are present
    """
    has_lines = False
    has_icons = False
    has_fills = False
    has_text = None
    txfmt = []

    # Check for each style type
    for st in zstyle:
        # Filter out empty/zero values for analysis
        st = {k: v for k, v in st.items() if str(v).strip(" 0.")}

        if 'width' in st or 'pattern-image' in st:
            has_lines = True
        if st.get(
                'icon-image') != 'none' if 'icon-image' in st else False or 'symbol-shape' in st or 'symbol-image' in st:
            has_icons = True
        if st.get('fill-color') != 'none' if 'fill-color' in st else False:
            has_fills = True

    # Collect unique text styles
    for st in zstyle:
        text_value = st.get('text')
        if text_value and text_value != 'none' and text_value not in txfmt:
            txfmt.append(text_value)
            if has_text is None:
                has_text = []
            has_text.append(st)

    return StyleAnalysis(
        has_lines=has_lines,
        has_icons=has_icons,
        has_fills=has_fills,
        has_text=has_text
    )


def _create_draw_element(zoom: int, runtime_conditions: Optional[List]) -> DrawElementDict:
    """Create and initialize a draw element dictionary."""
    dr_element = {
        'scale': zoom,
        'apply_if': [],
        'lines': [],
        'area': None,
        'symbol': None,
        'caption': None,
        'circle': None,
        'path_text': None,
        'shield': None
    }

    if runtime_conditions:
        for rc in runtime_conditions:
            dr_element['apply_if'].append(str(rc))

    return dr_element


class ProcessingResult(NamedTuple):
    """Result of processing a single style."""
    has_icons: bool
    has_text: Optional[List[Dict[str, str]]]
    has_fills: bool


def _process_single_style(st: Dict[str, str], zstyle: List[Dict[str, str]],
                          style_processor: StyleProcessor, dr_element: DrawElementDict,
                          analysis: StyleAnalysis,
                          zoom: int, cl: str, colors: Set[int]) -> ProcessingResult:
    """
    Process a single style rule and update the draw element dictionary.

    NOTE: This function has side effects - it MUTATES dr_element by adding
    lines, areas, symbols, captions, etc. based on the style rules.

    Args:
        st: Single style rule dictionary
        zstyle: Complete list of style rules (for context)
        style_processor: Style processor instance
        dr_element: Draw element dictionary (MUTATED IN-PLACE)
        analysis: Analysis results of what style types are present
        zoom: Current zoom level
        cl: Classification name
        colors: Set of color values

    Returns:
        ProcessingResult with updated flags for icons, text, and fills
    """
    has_lines = analysis.has_lines
    has_fills = analysis.has_fills
    has_icons = analysis.has_icons
    has_text = analysis.has_text

    # Process casing and area borders together (they're related)
    if st.get('casing-width') not in (None, 0) or st.get('casing-width-add') is not None:
        is_area_st = 'fill-color' in st

        # Process casing lines
        casing_lines = style_processor.process_casing(st, zstyle, has_lines, has_fills, zoom, cl)
        dr_element['lines'].extend(casing_lines)

        # Process casing border for areas
        # In protobuf, dr_element.area.border can be set without dr_element.area.color
        # Area borders only have width and color, NOT cap/join (those are only for lines)
        if has_fills and is_area_st and float(st.get('fill-opacity', 1)) > 0:
            # Ensure area dict exists (even if fill color not set yet)
            if dr_element['area'] is None:
                dr_element['area'] = {
                    'color': 0,  # Will be set by process_area_rule if fill-color exists
                    'priority': 0,
                    'border': None
                }

            # Set the border (width and color ONLY, no cap/join)
            dr_element['area']['border'] = {
                'width': st.get('casing-width', 0),
                'color': mwm_encode_color(colors, st, "casing"),
                'dashdot': None  # Could have dashdot, but not cap/join
            }

    # Process lines
    if has_lines:
        line_rules = style_processor.process_line_rules(st, zoom, cl)
        dr_element['lines'].extend(line_rules)

    # Process shield
    style_processor.process_shield_rule(st, zoom, cl, dr_element)

    # Process icons and circles
    has_icons = style_processor.process_icon_and_circle(st, zoom, cl, dr_element, has_icons)

    # Process text/captions
    has_text = style_processor.process_text_rules(st, has_text, zoom, cl, dr_element)

    # Process area fills
    has_fills = style_processor.process_area_rule(st, zoom, cl, dr_element, has_fills)

    return ProcessingResult(
        has_icons=has_icons,
        has_text=has_text,
        has_fills=has_fills
    )


class ClassificationProcessingResult(NamedTuple):
    """Result of processing a classification."""
    dr_cont: Optional[ClassificationDict]
    visstring: List[str]


def _process_classification_result(result: StyleQueryResult,
                                   options: Values, classificator_mgr: ClassificatorManager,
                                   style_processor: StyleProcessor, colors: Set[int],
                                   dr_cont: Optional[ClassificationDict], visstring: List[str],
                                   all_draw_elements: Set[str],
                                   visibility: Dict[str, str]) -> ClassificationProcessingResult:
    """
    Process a single classification result and generate draw element.

    Args:
        result: StyleQueryResult containing style data for one zoom level
        options: Command-line options
        classificator_mgr: Classification manager
        style_processor: Style processor instance
        colors: Set of color values
        dr_cont: Current classification container (or None for new)
        visstring: Visibility string array
        all_draw_elements: Set of unique draw element strings (for deduplication)
        visibility: Visibility dictionary

    Returns:
        ClassificationProcessingResult with updated dr_cont and visstring
    """
    cl = result.cl
    zoom = result.zoom
    runtime_conditions = result.runtime_conditions
    zstyle = result.zstyle

    # Sort rules by object-id for consistent ordering
    # Priority order:
    # 1. Rules with text and custom object-id (not ::default)
    # 2. Rules with text='none'
    # 3. All other rules
    def rule_sort_key(dict_):
        first = 0
        if dict_.get('text'):
            if str(dict_.get('object-id')) != '::default':
                first = 1  # Custom object-id rules come first
            if str(dict_.get('text')) == 'none':
                first = 2  # text='none' rules come last
        return (first, dict_.get('object-id'))

    zstyle.sort(key=rule_sort_key)

    if len(zstyle) == 0:
        return ClassificationProcessingResult(dr_cont, visstring)

    # Analyze style types
    analysis = _analyze_style_types(zstyle)

    if not (analysis.has_lines or analysis.has_text or analysis.has_fills or analysis.has_icons):
        return ClassificationProcessingResult(dr_cont, visstring)

    visstring[zoom] = "1"

    if zoom == 0:
        return ClassificationProcessingResult(dr_cont, visstring)

    # Create and populate draw element
    dr_element = _create_draw_element(zoom, runtime_conditions)

    # Process all style rules
    for st in zstyle:
        result = _process_single_style(
            st, zstyle, style_processor, dr_element,
            analysis,
            zoom, cl, colors
        )
        # Update analysis with results from processing
        analysis = StyleAnalysis(
            has_lines=analysis.has_lines,
            has_fills=result.has_fills,
            has_icons=result.has_icons,
            has_text=result.has_text
        )

    # Add draw element if unique
    # Use JSON for consistent string representation regardless of dict order
    str_dr_element = dr_cont['name'] + "/" + json.dumps(dr_element, sort_keys=True)
    if str_dr_element not in all_draw_elements:
        all_draw_elements.add(str_dr_element)
        dr_cont['elements'].append(dr_element)

    return ClassificationProcessingResult(dr_cont, visstring)


class ProcessStylesResult(NamedTuple):
    """Result of processing all styles."""
    drules_data: DrulesDataDict
    visibility: Dict[str, str]
    validation_errors: int


def process_styles(options: Values, style_obj: MapCSS, classificator_mgr: ClassificatorManager,
                   priority_mgr: PriorityManager, visibility_tracker: VisibilityTracker,
                   colors: Set[int],
                   add_pattern_func: Callable[[List[float]], None],
                   use_multiprocessing: bool = True) -> ProcessStylesResult:
    """
    Process all styles and generate drules.

    Args:
        options: Command line options
        style_obj: MapCSS style object
        classificator_mgr: ClassificatorManager instance
        priority_mgr: PriorityManager instance
        visibility_tracker: VisibilityTracker instance
        file_io_mgr: FileIOManager instance
        colors: Set of colors
        add_pattern_func: Function to add patterns
        use_multiprocessing: Whether to use multiprocessing (default: True)

    Returns:
        ProcessStylesResult with drules_data, visibility, and validation_errors
    """
    # Set global for multiprocessing workers (avoids pickling large object)
    global style
    style = style_obj

    # Extract and add style colors
    style_colors = _extract_style_colors(style_obj, colors)

    # Initialize drules data structure (dictionary, not protobuf)
    drules_data: DrulesDataDict = {
        'colors': [],
        'classifications': []
    }
    _add_colors_to_drules(drules_data, style_colors)

    # Setup multiprocessing or use serial map
    pool_context = None
    if use_multiprocessing:
        try:
            set_start_method('fork', force=False)
        except RuntimeError:
            # Already set, ignore
            pass
        pool_context = Pool()
        mapper = pool_context.imap
    else:
        mapper = map

    # Create style processor
    style_processor = StyleProcessor(priority_mgr, visibility_tracker, colors, add_pattern_func)

    try:
        # Process all classifications
        dr_cont: Optional[ClassificationDict] = None
        all_draw_elements: Set[str] = set()
        visibility: Dict[str, str] = {}

        for results in mapper(query_style,
                              (StyleQueryArgs(cl, classificator_mgr.classificator[cl], options.minzoom, options.maxzoom)
                               for cl in classificator_mgr.class_order)):
            for result in results:
                # Handle classification changes
                if dr_cont is not None and dr_cont['name'] != result.cl:
                    if dr_cont['elements']:
                        drules_data['classifications'].append(dr_cont)
                    visibility["world|" + classificator_mgr.class_tree[dr_cont['name']] + "|"] = "".join(visstring)
                    dr_cont = None

                if dr_cont is None:
                    dr_cont = {
                        'name': result.cl,
                        'elements': []
                    }
                    visstring = ["0"] * (options.maxzoom - options.minzoom + 1)

                # Process this classification result
                processing_result = _process_classification_result(
                    result, options, classificator_mgr, style_processor,
                    colors, dr_cont, visstring, all_draw_elements, visibility
                )
                dr_cont = processing_result.dr_cont
                visstring = processing_result.visstring

        # Add last classification
        if dr_cont is not None:
            if dr_cont['elements']:
                drules_data['classifications'].append(dr_cont)
            visibility["world|" + classificator_mgr.class_tree[dr_cont['name']] + "|"] = "".join(visstring)

    finally:
        # Ensure pool is cleaned up
        if pool_context is not None:
            pool_context.close()
            pool_context.join()

    return ProcessStylesResult(
        drules_data=drules_data,
        visibility=visibility,
        validation_errors=style_processor.validation_errors_count
    )


def main(options=None) -> None:
    """Main entry point for drules generation.

    Args:
        options: Pre-built options object (e.g. from tests). If None, options are
                 parsed from command-line arguments.
    """
    # Setup logging
    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
        datefmt='%Y-%m-%d %H:%M:%S'
    )

    if options is None:
        parser = OptionParser()
        parser.add_option("-s", "--stylesheet", dest="filename",
                          help="read MapCSS stylesheet from FILE", metavar="FILE")
        parser.add_option("-f", "--minzoom", dest="minzoom", default=0, type="int",
                          help="minimal available zoom level", metavar="ZOOM")
        parser.add_option("-t", "--maxzoom", dest="maxzoom", default=20, type="int",
                          help="maximal available zoom level", metavar="ZOOM")
        parser.add_option("-o", "--output-file", dest="outfile", default="-",
                          help="output filename", metavar="FILE")
        parser.add_option("-x", "--txt", dest="txt", action="store_true",
                          help="create a text file for output", default=False)
        parser.add_option("-p", "--priorities-path", dest="priorities_path",
                          help="path to priorities *.prio.txt files", metavar="PATH")
        parser.add_option("-d", "--data-path", dest="data",
                          help="path to mapcss-mapping.csv and other files", metavar="PATH")
        parser.add_option("-v", "--verbose", dest="verbose", action="store_true",
                          help="enable verbose logging", default=False)
        parser.add_option("--multiprocessing", dest="multiprocessing", action="store_true",
                          help="enable multiprocessing", default=True)

        (options, args) = parser.parse_args()

        # Validate required options
        if not options.filename:
            parser.error("MapCSS stylesheet filename is required (-s/--stylesheet)")
        if not os.path.isfile(options.filename):
            parser.error(f"MapCSS stylesheet file not found: {options.filename}")
        if options.outfile == "-":
            parser.error("Please specify base output path (-o/--output-file)")
        if not options.priorities_path:
            parser.error("A path to priorities *.prio.txt files is required (-p/--priorities-path)")
        if not os.path.isdir(options.priorities_path):
            parser.error(f"Priorities path is not a directory: {options.priorities_path}")

    # Apply defaults for any attributes not set by the caller
    if not hasattr(options, 'verbose'):
        options.verbose = False
    if not hasattr(options, 'multiprocessing'):
        options.multiprocessing = True
    if not hasattr(options, 'txt'):
        options.txt = False
    if not hasattr(options, 'minzoom'):
        options.minzoom = 0
    if not hasattr(options, 'maxzoom'):
        options.maxzoom = 20

    # Adjust logging level if verbose
    if options.verbose:
        logging.getLogger().setLevel(logging.DEBUG)
        logger.debug("Verbose logging enabled")

    # Normalize and validate paths
    options.priorities_path = os.path.normpath(os.path.abspath(options.priorities_path))
    options.filename = os.path.normpath(os.path.abspath(options.filename))
    options.outfile = os.path.normpath(os.path.abspath(options.outfile))

    # Determine data directory
    if options.data and os.path.isdir(options.data):
        ddir = options.data
    else:
        ddir = os.path.dirname(options.outfile)

    logger.info("Starting drules generation")
    logger.debug(f"Output path: {options.outfile}")
    logger.debug(f"Data directory: {ddir}")
    logger.debug(f"Priorities path: {options.priorities_path}")

    # Initialize managers
    logger.info("Initializing managers...")
    prio_ranges = get_prio_ranges()
    priority_mgr = PriorityManager(prio_ranges)
    classificator_mgr = ClassificatorManager()
    file_io_mgr = FileIOManager(ddir)
    visibility_tracker = VisibilityTracker()

    # Load data files
    logger.info("Loading data files...")
    colors = file_io_mgr.load_colors()
    logger.debug(f"Loaded {len(colors)} colors")
    patterns, add_pattern = file_io_mgr.load_patterns()
    logger.debug(f"Loaded {len(patterns)} patterns")

    # Parse classificator
    logger.info("Parsing classificator...")
    unique_types_check = classificator_mgr.parse_mapcss_mapping(ddir)
    logger.debug(f"Parsed {len(unique_types_check)} unique types")

    # Load priorities
    logger.info("Loading priorities...")
    output = ''
    for prio_range in prio_ranges.keys():
        priority_mgr.load_priorities(prio_range, options.priorities_path, unique_types_check, compress=False)
        output += f'{"" if not output else ", "}{len(prio_ranges[prio_range]["priorities"])} {prio_range}'
    logger.info(f'Loaded priorities: {output}')

    del unique_types_check

    # Get tags
    mapcss_static_tags = classificator_mgr.get_mapcss_static_tags()
    mapcss_dynamic_tags = load_mapcss_dynamic_tags(ddir)
    logger.debug(f"Loaded {len(mapcss_static_tags)} static tags, {len(mapcss_dynamic_tags)} dynamic tags")

    # Parse MapCSS stylesheet
    logger.info("Parsing MapCSS stylesheet...")
    style = MapCSS(options.minzoom, options.maxzoom)
    style.parse(clamp=False, stretch=LAYER_PRIORITY_RANGE,
                filename=options.filename, static_tags=mapcss_static_tags,
                dynamic_tags=mapcss_dynamic_tags)

    # Build optimization tree
    logger.info("Building optimization tree...")
    clname_cltag_unique = set()
    for cl in classificator_mgr.class_order:
        clname = cl if cl.find('-') == -1 else cl[:cl.find('-')]
        cltag = next(iter(classificator_mgr.classificator[cl].keys()))
        clname_cltag = clname + '$' + cltag
        if clname_cltag not in clname_cltag_unique:
            clname_cltag_unique.add(clname_cltag)
            style.build_choosers_tree(clname, "line", cltag)
            style.build_choosers_tree(clname, "area", cltag)
            style.build_choosers_tree(clname, "node", cltag)

    style.finalize_choosers_tree()
    logger.debug(f"Built optimization tree for {len(clname_cltag_unique)} type combinations")

    if options.multiprocessing:
        logger.debug("Multiprocessing: enabled")
    else:
        logger.info("Multiprocessing: disabled (running in single process mode)")

    # Process styles and generate drules
    logger.info("Processing styles...")
    result = process_styles(
        options, style, classificator_mgr, priority_mgr,
        visibility_tracker, colors, add_pattern,
        use_multiprocessing=options.multiprocessing
    )
    drules_data = result.drules_data
    visibility = result.visibility
    validation_errors = result.validation_errors

    # Validate
    visibility_tracker.validate_visibilities(options.maxzoom)

    if validation_errors:
        logger.error(f'FAILED to write regenerated drules files!')
        logger.error(f'There are {validation_errors} validation errors (see in the log above).')
        logger.error('Fix all errors first and re-run.')
        exit(1)

    # Dump priorities - reformats with zoom visibility comments
    logger.info("Dumping priorities...")
    output = ''
    for prio_range in prio_ranges.keys():
        priority_mgr.dump_priorities(prio_range, options.priorities_path, options.maxzoom,
                                     visibility_tracker.get_visibilities())
        output += f'{"" if not output else ", "}{len(prio_ranges[prio_range]["priorities"])} {prio_range}'
    logger.info(f'Re-formatted priorities files: {output}')

    # Serialize to protobuf (this is the ONLY place protobuf is used)
    logger.info("Serializing to protobuf format...")
    try:
        serializer = DrulesSerializer()
        drules_binary = serializer.serialize(drules_data)
    except Exception as e:
        logger.error(f"Failed to serialize drules to protobuf: {e}")
        raise

    # Write binary output
    logger.info("Writing binary output...")
    with open(os.path.join(options.outfile + '.bin'), "wb") as drules_bin:
        drules_bin.write(drules_binary)
    logger.debug(f"Wrote binary file: {options.outfile}.bin")

    if options.txt:
        logger.info("Writing text output...")
        drules_text = serializer.serialize_to_text(drules_data)
        with open(os.path.join(options.outfile + '.txt'), "w") as drules_txt:
            drules_txt.write(drules_text)
        logger.debug(f"Wrote text file: {options.outfile}.txt")

    # Write visibility and classificator files
    logger.info("Writing visibility files...")
    file_io_mgr.save_visibility(visibility, options.maxzoom)

    # Write colors and patterns
    logger.info("Writing colors and patterns...")
    file_io_mgr.save_colors(colors)
    file_io_mgr.save_patterns(patterns)
    logger.debug(f"Wrote {len(colors)} colors and {len(patterns)} patterns")

    logger.info("Drules generation completed successfully!")


if __name__ == '__main__':
    main()
