from mapcss import MapCSS
from optparse import OptionParser
import os
import csv
import sys
import functools
from itertools import chain
from multiprocessing import Pool, set_start_method
from collections import OrderedDict
import mapcss.webcolors

whatever_to_hex = mapcss.webcolors.webcolors.whatever_to_hex
whatever_to_cairo = mapcss.webcolors.webcolors.whatever_to_cairo

PROFILE = False
MULTIPROCESSING = True

# If path to the protobuf EGG is specified then apply it before import drules_struct_pb2
PROTOBUF_EGG_PATH = os.environ.get("PROTOBUF_EGG_PATH")
if PROTOBUF_EGG_PATH:
    # another version of protobuf may be installed, override it
    for i in range(len(sys.path)):
        if -1 != sys.path[i].find("protobuf-"):
            sys.path[i] = PROTOBUF_EGG_PATH
    sys.path.append(PROTOBUF_EGG_PATH)

from drules_struct_pb2 import *

WIDTH_SCALE = 1.0

# Priority values defined in *.prio.txt files are adjusted
# to fit into the following "priorities ranges":
# [-10000; 10000): overlays (icons, captions...)
# [0; 1000)      : FG - foreground areas and lines
# [-1000; 0)     : BG-top - water, linear and areal, rendered just on top of landcover
# (-2000; -1000) : BG-by-size - landcover areas, later in core sorted by their bbox size
# The core renderer then re-adjusts those ranges as necessary to accomodate
# for special behavior and features' layer=* values.
# See drape_frontend/stylist.cpp for the details of layering logic.

# Priority range for area and line drules. Should be same as drule::kLayerPriorityRange.
LAYER_PRIORITY_RANGE = 1000
# Should be same as drule::kOverlaysMaxPriority. The overlays range is [-kOverlaysMaxPriority; kOverlaysMaxPriority),
# negative values are used for optional captions which are below all other overlays.
OVERLAYS_MAX_PRIORITY = 10000

# Drules are arranged into following ranges.
PRIO_OVERLAYS = 'overlays'
PRIO_FG = 'FG'
PRIO_BG_TOP = 'BG-top'
PRIO_BG_BY_SIZE = 'BG-by-size'

prio_ranges = {
    PRIO_OVERLAYS: {'pos': 4, 'base': 0, 'priorities': {}},
    PRIO_FG: {'pos': 3, 'base': 0, 'priorities': {}},
    PRIO_BG_TOP: {'pos': 2, 'base': -1000, 'priorities': {}},
    PRIO_BG_BY_SIZE: {'pos': 1, 'base': -2000, 'priorities': {}},
}

visibilities = {}

prio_ranges[PRIO_OVERLAYS]['comment'] = '''
Overlays (icons, captions, path texts and shields) are rendered on top of all the geometry (lines, areas).
Overlays don't overlap each other, instead the ones with higher priority displace the less important ones.
Optional captions (which have an icon) are displayed only if there are no other overlays in their way
(technically their priorities are automatically put below all other overlays in the same order as corresponding icons).
House numbers are hardcoded to have the lowest priority.
'''

prio_ranges[PRIO_OVERLAYS]['comment'] = f'''
Overlays (icons, captions, path texts and shields) are rendered on top of all the geometry (lines, areas).
Overlays don't overlap each other, instead the ones with higher priority displace the less important ones.
Optional captions (which have an icon) are displayed only if there are no other overlays in their way
(technically, max overlays priority value ({OVERLAYS_MAX_PRIORITY}) is subtracted from their priorities automatically).
'''

prio_ranges[PRIO_FG]['comment'] = '''
FG geometry: foreground lines and areas (e.g. buildings) are rendered always below overlays
and always on top of background geometry (BG-top & BG-by-size) even if a foreground feature
is layer=-10 (as tunnels should be visibile over landcover and water).
'''
prio_ranges[PRIO_BG_TOP]['comment'] = '''
BG-top geometry: background lines and areas that should be always below foreground ones
(including e.g. layer=-10 underwater tunnels), but above background areas sorted by size (BG-by-size),
because ordering by size doesn't always work with e.g. water mapped over a forest,
so water should be on top of other landcover always, but linear waterways should be hidden beneath it.
Still, e.g. a layer=-1 BG-top feature will be rendered under a layer=0 BG-by-size feature
(so areal water tunnels are hidden beneath other landcover area) and a layer=1 landcover areas
are displayed above layer=0 BG-top.
'''
prio_ranges[PRIO_BG_BY_SIZE]['comment'] = '''
BG-by-size geometry: background areas rendered below BG-top and everything else.
Smaller areas are rendered above larger ones (area's size is estimated as the size of its' bounding box).
So effectively priority values of BG-by-size areas are not used at the moment.
But we might use them later for some special cases, e.g. to determine a main area type of a multi-type feature.
Keep them in a logical importance order please.
'''

COMMENT_AUTOFORMAT = '''This file is automatically re-formatted and re-sorted in priorities descending order
when generate_drules.sh is run. Custom formatting and comments are not preserved.
'''

COMMENT_RANGES_OVERVIEW = '''
Priorities ranges' rendering order overview:
- overlays (icons, captions...)
- FG: foreground areas and lines
- BG-top: water (linear and areal)
- BG-by-size: landcover areas sorted by their size
'''

def to_boolean(s):
    s = s.lower()
    if s == "true" or s == "yes":
        return True, True # Valid, True
    elif s == "false" or s == "no":
        return True, False # Valid, False
    else:
        return False, False # Invalid

def mwm_encode_color(colors, st, prefix='', default='black'):
    if prefix:
        prefix += "-"
    opacity = hex(255 - int(255 * float(st.get(prefix + "opacity", 1))))
    color = whatever_to_hex(st.get(prefix + 'color', default))[1:]
    result = int(opacity + color, 16)
    colors.add(result)
    return result

def mwm_encode_image(st, prefix='icon', bgprefix='symbol'):
    if prefix:
        prefix += "-"
    if bgprefix:
        bgprefix += "-"
    if prefix + "image" not in st:
        return False
    # strip last ".svg"
    handle = st.get(prefix + "image")[:-4]
    return handle, handle


def query_style(args):
    global style
    cl, cltags, minzoom, maxzoom = args
    clname = cl if cl.find('-') == -1 else cl[:cl.find('-')]

    cltags["name"] = "name"
    cltags["addr:housenumber"] = "addr:housenumber"
    cltags["addr:housename"] = "addr:housename"
    cltags["ref"] = "ref"
    cltags["int_name"] = "int_name"
    cltags["addr:flats"] = "addr:flats"

    results = []
    for zoom in range(minzoom, maxzoom + 1):
        all_runtime_conditions_arr = []
        # Get runtime conditions which are used for class 'cl' on zoom 'zoom'
        if "area" not in cltags:
            all_runtime_conditions_arr.extend(style.get_runtime_rules(clname, "line", cltags, zoom))
        all_runtime_conditions_arr.extend(style.get_runtime_rules(clname, "area", cltags, zoom))
        if "area" not in cltags:
            all_runtime_conditions_arr.extend(style.get_runtime_rules(clname, "node", cltags, zoom))

        runtime_conditions_arr = []
        if len(all_runtime_conditions_arr) == 0:
            # If there is no runtime conditions, do not filter style by runtime conditions
            runtime_conditions_arr.append(None)
        elif len(all_runtime_conditions_arr) == 1:
            runtime_conditions_arr = all_runtime_conditions_arr
        else:
            # Keep unique conditions only
            runtime_conditions_arr.append(all_runtime_conditions_arr.pop(0))
            for new_rt_conditions in all_runtime_conditions_arr:
                conditions_unique = True
                for rt_conditions in runtime_conditions_arr:
                    if new_rt_conditions == rt_conditions:
                        conditions_unique = False
                        break
                if conditions_unique:
                    runtime_conditions_arr.append(new_rt_conditions)

        for runtime_conditions in runtime_conditions_arr:
            zstyle = {}

            # Get style for class 'cl' on zoom 'zoom' with corresponding runtime conditions
            if "area" not in cltags:
                linestyle = style.get_style_dict(clname, "line", cltags, zoom, olddict=zstyle, filter_by_runtime_conditions=runtime_conditions)
                zstyle = linestyle
            areastyle = style.get_style_dict(clname, "area", cltags, zoom, olddict=zstyle, filter_by_runtime_conditions=runtime_conditions)
            zstyle = areastyle
            if "area" not in cltags:
                nodestyle = style.get_style_dict(clname, "node", cltags, zoom, olddict=zstyle, filter_by_runtime_conditions=runtime_conditions)
                zstyle = nodestyle

            results.append((cl, zoom, runtime_conditions, list(zstyle.values())))
    return results

def get_priorities_filename(prio_range, path):
    return os.path.join(path, f'priorities_{prio_ranges[prio_range]["pos"]}_{prio_range}.prio.txt')

def load_priorities(prio_range, path, classif, compress = False):
    def print_warning(msg):
        print(f'WARNING: {msg} in {fname}:\n\t{line}')

    priority_max = OVERLAYS_MAX_PRIORITY if prio_range == PRIO_OVERLAYS else LAYER_PRIORITY_RANGE
    fname = get_priorities_filename(prio_range, path)
    with open(fname, 'r') as f:
        group = []
        for line in f:
            line = line.strip()
            # Strip comments.
            line = line.split('#', 1)[0].strip()
            if not line:
                continue
            tokens = line.split()
            if len(tokens) > 2:
                print_warning('skipping malformed line')
                continue
            if tokens[0] == "===":
                try:
                    priority = int(tokens[1])
                except ValueError:
                    print_warning('skipping invalid priority value')
                else:
                    if priority >= 0 and priority < priority_max:
                        if len(group):
                            for key in group:
                                prio_ranges[prio_range]['priorities'][key] = priority
                        else:
                            print_warning('skipping empty priority group')
                    else:
                        print_warning(f'skipping out of [0;{priority_max}) range priority value')
                group = []
            else:
                cl = tokens[0]
                object_id = ''
                oid_pos = cl.find('::')
                if oid_pos != -1:
                    object_id = cl[oid_pos:]
                    cl = cl[0:oid_pos]
                if cl not in classif:
                    print_warning('unknown classificator type')
                key = (cl, object_id)
                if key in prio_ranges[prio_range]['priorities']:
                    print_warning(f'overriding previously set priority value {prio_ranges[prio_range]["priorities"][key]}')
                group.append(key)

        if len(group):
            line = group
            print_warning(f'skipping last types groups with no priority set')

    if prio_range == PRIO_OVERLAYS:
        for key in prio_ranges[PRIO_OVERLAYS]['priorities'].keys():
            main_prio_id = None
            if key[1].startswith('caption'):
                main_prio_id = (key[0], key[1].replace('caption', 'icon'))
            if key[1].startswith('pathtext'):
                main_prio_id = (key[0], key[1].replace('pathtext', 'shield'))
            if main_prio_id is not None and main_prio_id in prio_ranges[PRIO_OVERLAYS]['priorities']:
                main_prio = prio_ranges[PRIO_OVERLAYS]['priorities'][main_prio_id]
                if prio_ranges[PRIO_OVERLAYS]['priorities'][key] > main_prio:
                    print(f'WARNING: {key} priority is higher than {main_prio_id}, making it equal')
                    prio_ranges[PRIO_OVERLAYS]['priorities'][key] = main_prio

    if compress:
        print(f'Compressing {prio_range} priorities into a (0;{priority_max}) range:')
        unique_prios = set(prio_ranges[prio_range]['priorities'].values())
        print(f'\tunique priorities values: {len(unique_prios)}')
        # Keep gaps at the range borders.
        base_idx = 1
        if 0 not in unique_prios:
            base_idx = 0
            unique_prios.add(0)
        unique_prios.add(priority_max)
        step = min(priority_max / len(unique_prios), 10)
        print(f'\tnew step between priorities: {step}')
        unique_prios = sorted(unique_prios)
        for prio_id in prio_ranges[prio_range]['priorities'].keys():
            idx = unique_prios.index(prio_ranges[prio_range]['priorities'][prio_id])
            prio_ranges[prio_range]['priorities'][prio_id] = int(step * (base_idx + idx))


def store_visibility(cl, dr_type, object_id, zoom):
    if object_id == '::default':
        object_id = ''
    if cl not in visibilities:
        visibilities[cl] = {}
    if dr_type not in visibilities[cl]:
        visibilities[cl][dr_type] = {}
    if object_id not in visibilities[cl][dr_type]:
        visibilities[cl][dr_type][object_id] = set()
    visibilities[cl][dr_type][object_id].add(zoom)


def prettify_zooms(zooms, maxzoom):

    def add_zrange(first, prev, result, maxzoom):
        first = str(first)
        prev = str(prev)
        if first == prev:
            zrange = first
        elif prev == str(maxzoom):
            zrange = first + '-'
        else:
            zrange = first + '-' + prev
        if result != '':
            result += ','
        result += zrange
        return result

    zooms = sorted(zooms)
    first = zooms.pop(0)
    prev = first
    result = ''
    for zoom in zooms:
        if zoom == prev + 1:
            prev = zoom
        else:
            result = add_zrange(first, prev, result, maxzoom)
            first = zoom
            prev = zoom
    return 'z' + add_zrange(first, prev, result, maxzoom)


def validate_visibilities(maxzoom):
    for cl, dr_types in visibilities.items():
        for dr_type, object_ids in dr_types.items():
            for object_id, zooms in object_ids.items():
                zoom_range = prettify_zooms(zooms, maxzoom)
                if zoom_range.find(',') != -1:
                    print(f'WARNING: non-contiguous visibility range {zoom_range} for {cl} {dr_type}{object_id}')

                if dr_type == 'caption' and 'icon' in dr_types and object_id in dr_types['icon']:
                    icon_zooms = sorted(dr_types['icon'][object_id])
                    if sorted(zooms)[0] < icon_zooms[0]:
                        print(f'WARNING: caption {zoom_range} appears before icon {prettify_zooms(icon_zooms, maxzoom)}'
                              f' for {cl}{object_id}')


def dump_priorities(prio_range, path, maxzoom):
    with open(get_priorities_filename(prio_range, path), 'w') as outfile:
        comment = COMMENT_AUTOFORMAT + prio_ranges[prio_range]['comment'] + COMMENT_RANGES_OVERVIEW
        for s in comment.splitlines():
            outfile.write(f'# {s}'.strip() + '\n')
        outfile.write('\n')

        if len(prio_ranges[prio_range]['priorities']):
            prios = sorted(prio_ranges[prio_range]['priorities'].items(),
                           key = lambda item: (OVERLAYS_MAX_PRIORITY - item[1], item[0][0], item[0][1]))
            group_prio = prios[0][1]
            group = ''
            for p in prios:
                if p[1] != group_prio:
                    outfile.write(f'{group}=== {group_prio}\n\n')
                    group_prio = p[1]
                    group = ''
                cl = p[0][0]
                object_id = p[0][1]

                line_drules = ''
                other_drules = ''
                if cl in visibilities:
                    for dr_type in sorted(visibilities[cl].keys()):
                        for oid in sorted(visibilities[cl][dr_type].keys()):
                            dr_zoom = dr_type + oid if oid else dr_type
                            dr_zoom += ' ' + prettify_zooms(visibilities[cl][dr_type][oid], maxzoom)
                            # Drules matching this prio_range and object_id.
                            if (oid == object_id and
                                ((prio_range == PRIO_OVERLAYS and dr_type in ('icon', 'caption', 'shield', 'pathtext')) or
                                 (prio_range in (PRIO_FG, PRIO_BG_TOP) and dr_type in ('line', 'area')) or
                                 (prio_range == PRIO_BG_BY_SIZE and dr_type == 'area'))):
                                    if line_drules:
                                        line_drules += ' and '
                                    line_drules += dr_zoom
                            else:
                                # Drules from other prio_ranges or with other object_ids.
                                if other_drules:
                                    other_drules += ', '
                                other_drules += dr_zoom
                if object_id:
                    cl += object_id
                if not line_drules:
                    print(f'WARNING: priority is defined, but no drules for {cl}')

                info = ''
                if line_drules or other_drules:
                    info = f' # {line_drules}'
                    if other_drules:
                        info += f' (also has {other_drules})'
                group += f'{cl:50} {info}\n'
            outfile.write(f'{group}=== {group_prio}\n')

def get_drape_priority(cl, dr_type, object_id):
    if object_id == '::default':
        object_id = ''
    prio_id = (cl, object_id)

    ranges_to_check = (PRIO_OVERLAYS, )
    if dr_type == 'line':
        ranges_to_check = (PRIO_FG, PRIO_BG_TOP)
    elif dr_type == 'area':
        ranges_to_check = (PRIO_BG_BY_SIZE, PRIO_BG_TOP, PRIO_FG)
    for r in ranges_to_check:
        if prio_id in prio_ranges[r]['priorities']:
            return prio_ranges[r]['priorities'][prio_id] + prio_ranges[r]['base']

    print(f'WARNING: priority is not set for {prio_id}')
    return 0

def komap_mapswithme(options):
    if options.data and os.path.isdir(options.data):
        ddir = options.data
    else:
        ddir = os.path.dirname(options.outfile)

    classificator = {}
    class_order = []
    class_tree = {}

    colors_file_name = os.path.join(ddir, 'colors.txt')
    colors = set()
    if os.path.exists(colors_file_name):
        colors_in_file = open(colors_file_name, "r")
        for colorLine in colors_in_file:
            colors.add(int(colorLine))
        colors_in_file.close()

    patterns = []
    def addPattern(dashes):
        if dashes and dashes not in patterns:
            patterns.append(dashes)

    patterns_file_name = os.path.join(ddir, 'patterns.txt')
    if os.path.exists(patterns_file_name):
        patterns_in_file = open(patterns_file_name, "r")
        for patternsLine in patterns_in_file:
            addPattern([float(x) for x in patternsLine.split()])
        patterns_in_file.close()

    # Build classificator tree from mapcss-mapping.csv file
    types_file = open(os.path.join(ddir, 'types.txt'), "w")

    # The mapcss-mapping.csv format is described inside the file itself.
    cnt = 1
    unique_types_check = set()
    for row in csv.reader(open(os.path.join(ddir, 'mapcss-mapping.csv')), delimiter=';'):
        if len(row) <= 1 or row[0].startswith('#'):
            # Allow for empty lines and comment lines starting with '#'.
            continue
        if len(row) == 3:
            # Short format: type name, type id, x / replacement type name
            tag = row[0].replace('|', '=')
            obsolete = len(row[2].strip()) > 0
            row = (row[0], '[{0}]'.format(tag), 'x' if obsolete else '', 'name', 'int_name', row[1], row[2] if row[2] != 'x' else '')
        if len(row) != 7:
            raise Exception('Expecting 3 or 7 columns in mapcss-mapping: {0}'.format(';'.join(row)))

        if int(row[5]) < cnt:
            raise Exception('Wrong type id: {0}'.format(';'.join(row)))
        while int(row[5]) > cnt:
            print("mapswithme", file=types_file)
            cnt += 1
        cnt += 1

        cl = row[0].replace("|", "-")
        if cl in unique_types_check and row[2] != 'x':
            raise Exception('Duplicate type: {0}'.format(row[0]))
        pairs = [i.strip(']').split("=") for i in row[1].split(',')[0].split('[')]
        kv = OrderedDict()
        for i in pairs:
            if len(i) == 1:
                if i[0]:
                    if i[0][0] == "!":
                        kv[i[0][1:].strip('?')] = "no"
                    else:
                        kv[i[0].strip('?')] = "yes"
            else:
                kv[i[0]] = i[1]
        if row[2] != "x":
            classificator[cl] = kv
            class_order.append(cl)
            unique_types_check.add(cl)
            # Mark original type to distinguish it among replacing types.
            print("*" + row[0], file=types_file)
        else:
            # compatibility mode
            if row[6]:
                print(row[6], file=types_file)
            else:
                print("mapswithme", file=types_file)
        class_tree[cl] = row[0]
    class_order.sort()
    types_file.close()

    output = ''
    for prio_range in prio_ranges.keys():
        load_priorities(prio_range, options.priorities_path, unique_types_check, compress = False)
        output += f'{"" if not output else ", "}{len(prio_ranges[prio_range]["priorities"])} {prio_range}'
    print(f'Loaded priorities: {output}.')

    del unique_types_check

    # Get all mapcss static tags which are used in mapcss-mapping.csv
    # This is a dict with main_tag flags (True = appears first in types)
    mapcss_static_tags = {}
    for v in list(classificator.values()):
        for i, t in enumerate(v.keys()):
            mapcss_static_tags[t] = mapcss_static_tags.get(t, True) and i == 0

    # Get all mapcss dynamic tags from mapcss-dynamic.txt
    mapcss_dynamic_tags = set([line.rstrip() for line in open(os.path.join(ddir, 'mapcss-dynamic.txt'))])

    # Parse style mapcss
    global style
    style = MapCSS(options.minzoom, options.maxzoom)
    style.parse(clamp=False, stretch=LAYER_PRIORITY_RANGE,
                filename=options.filename, static_tags=mapcss_static_tags,
                dynamic_tags=mapcss_dynamic_tags)

    # Build optimization tree - class/zoom/type -> StyleChoosers
    clname_cltag_unique = set()
    for cl in class_order:
        clname = cl if cl.find('-') == -1 else cl[:cl.find('-')]
        # Get first tag of the class/type.
        cltag = next(iter(classificator[cl].keys()))
        clname_cltag = clname + '$' + cltag
        if clname_cltag not in clname_cltag_unique:
            clname_cltag_unique.add(clname_cltag)
            style.build_choosers_tree(clname, "line", cltag)
            style.build_choosers_tree(clname, "area", cltag)
            style.build_choosers_tree(clname, "node", cltag)

    style.finalize_choosers_tree()

    # Get colors section from style
    style_colors = {}
    raw_style_colors = style.get_colors()
    if raw_style_colors is not None:
        unique_style_colors = set()
        for k in list(raw_style_colors.keys()):
            unique_style_colors.add(k[:k.rindex('-')])
        for k in unique_style_colors:
            style_colors[k] = mwm_encode_color(colors, raw_style_colors, k)

    visibility = {}

    dr_linecaps = {'none': BUTTCAP, 'butt': BUTTCAP, 'round': ROUNDCAP}
    dr_linejoins = {'none': NOJOIN, 'bevel': BEVELJOIN, 'round': ROUNDJOIN}

    # Build drules tree

    drules = ContainerProto()
    dr_cont = None
    if MULTIPROCESSING:
        set_start_method('fork')  # Use fork with multiprocessing to share global variables among Python instances
        pool = Pool()
        imapfunc = pool.imap
    else:
        imapfunc = map

    if style_colors:
        for k, v in sorted(list(style_colors.items())):
            color_proto = ColorElementProto()
            color_proto.name = k
            color_proto.color = v
            color_proto.x = 0
            color_proto.y = 0
            drules.colors.value.extend([color_proto])

    all_draw_elements = set()

    for results in imapfunc(query_style, ((cl, classificator[cl], options.minzoom, options.maxzoom) for cl in class_order)):
        for result in results:
                cl, zoom, runtime_conditions, zstyle = result

                # First, sort rules by ::object-id in captions (primary, secondary, none ..)
                # then by other ::object-id in ascending order.
                def rule_sort_key(dict_):
                    first = 0
                    if dict_.get('text'):
                        if str(dict_.get('object-id')) != '::default':
                            first = 1
                        if str(dict_.get('text')) == 'none':
                            first = 2
                    return (first, dict_.get('object-id'))

                zstyle.sort(key = rule_sort_key)

                # For debug purpose.
                # if str(cl) == 'highway-path' and int(zoom) == 19:
                #     print(cl)
                #     print(zstyle)

                if dr_cont is not None and dr_cont.name != cl:
                    if dr_cont.element:
                        drules.cont.extend([dr_cont])
                    visibility["world|" + class_tree[dr_cont.name] + "|"] = "".join(visstring)
                    dr_cont = None

                if dr_cont is None:
                    dr_cont = ClassifElementProto()
                    dr_cont.name = cl
                    dr_lines_objects = {}

                    visstring = ["0"] * (options.maxzoom - options.minzoom + 1)

                if len(zstyle) == 0:
                    continue

                has_lines = False
                has_icons = False
                has_fills = False
                for st in zstyle:
                    st = dict([(k, v) for k, v in st.items() if str(v).strip(" 0.")])
                    if 'width' in st or 'pattern-image' in st:
                        has_lines = True
                    if 'icon-image' in st or 'symbol-shape' in st or 'symbol-image' in st:
                        has_icons = True
                    if 'fill-color' in st:
                        has_fills = True

                has_text = None
                txfmt = []
                for st in zstyle:
                    if st.get('text') and st.get('text') != 'none' and not st.get('text') in txfmt:
                        txfmt.append(st.get('text'))
                        if has_text is None:
                            has_text = []
                        has_text.append(st)

                if (not has_lines) and (not has_text) and (not has_fills) and (not has_icons):
                    continue

                visstring[zoom] = "1"

                if zoom == 0:
                    continue

                dr_element = DrawElementProto()
                dr_element.scale = zoom

                if runtime_conditions:
                    for rc in runtime_conditions:
                        dr_element.apply_if.append(str(rc))

                for st in zstyle:
                    if st.get('casing-width') not in (None, 0) or st.get('casing-width-add') is not None:  # and (st.get('width') or st.get('fill-color')):
                        is_area_st = 'fill-color' in st
                        if has_lines and not is_area_st and st.get('casing-linecap', 'butt') == 'butt':
                            dr_line = LineRuleProto()

                            base_width = st.get('width', 0)
                            if base_width == 0:
                                for wst in zstyle:
                                    if wst.get('width') not in (None, 0):
                                        # Rail bridge styles use width from ::dash object instead of ::default.
                                        if base_width == 0 or wst.get('object-id') != '::default':
                                            base_width = wst.get('width', 0)
                                # 'casing-width' has precedence over 'casing-width-add'.
                                if st.get('casing-width') in (None, 0):
                                    st['casing-width'] = base_width + st.get('casing-width-add')
                                    base_width = 0

                            dr_line.width = round((base_width + st.get('casing-width') * 2) * WIDTH_SCALE, 2)
                            dr_line.color = mwm_encode_color(colors, st, "casing")
                            dr_line.priority = get_drape_priority(cl, 'line', st.get('object-id'))
                            # Casing line should be rendered below the "main" line, hence priority -1 (but not less than base).
                            if (st.get('object-id') == '::default' and dr_line.priority != 0 and
                                dr_line.priority != prio_ranges[PRIO_FG]['base'] and
                                dr_line.priority != prio_ranges[PRIO_BG_TOP]['base']):
                                    store_visibility(cl, 'line', '(casing)', zoom)
                                    dr_line.priority -= 1
                            else:
                                store_visibility(cl, 'line', st.get('object-id'), zoom)
                            for i in st.get('casing-dashes', st.get('dashes', [])):
                                dr_line.dashdot.dd.extend([max(float(i), 1) * WIDTH_SCALE])
                            addPattern(dr_line.dashdot.dd)
                            dr_line.cap = dr_linecaps.get(st.get('casing-linecap', 'butt'), BUTTCAP)
                            dr_line.join = dr_linejoins.get(st.get('casing-linejoin', 'round'), ROUNDJOIN)
                            dr_element.lines.extend([dr_line])

                        if has_fills and is_area_st and float(st.get('fill-opacity', 1)) > 0:
                            dr_element.area.border.color = mwm_encode_color(colors, st, "casing")
                            dr_element.area.border.width = st.get('casing-width', 0) * WIDTH_SCALE

                        # Let's try without this additional line style overhead. Needed only for casing in road endings.
                        # if st.get('casing-linecap', st.get('linecap', 'round')) != 'butt':
                        #     dr_line = LineRuleProto()
                        #     dr_line.width = (st.get('width', 0) * WIDTH_SCALE) + (st.get('casing-width') * WIDTH_SCALE * 2)
                        #     dr_line.color = mwm_encode_color(colors, st, "casing")
                        #     dr_line.priority = -15000
                        #     dashes = st.get('casing-dashes', st.get('dashes', []))
                        #     dr_line.dashdot.dd.extend(dashes)
                        #     dr_line.cap = dr_linecaps.get(st.get('casing-linecap', 'round'), ROUNDCAP)
                        #     dr_line.join = dr_linejoins.get(st.get('casing-linejoin', 'round'), ROUNDJOIN)
                        #     dr_element.lines.extend([dr_line])

                    if has_lines:
                        if st.get('width'):
                            dr_line = LineRuleProto()
                            dr_line.width = (st.get('width', 0) * WIDTH_SCALE)
                            dr_line.color = mwm_encode_color(colors, st)
                            for i in st.get('dashes', []):
                                dr_line.dashdot.dd.extend([max(float(i), 1) * WIDTH_SCALE])
                            addPattern(dr_line.dashdot.dd)
                            dr_line.cap = dr_linecaps.get(st.get('linecap', 'butt'), BUTTCAP)
                            dr_line.join = dr_linejoins.get(st.get('linejoin', 'round'), ROUNDJOIN)
                            dr_line.priority = get_drape_priority(cl, 'line', st.get('object-id'))
                            store_visibility(cl, 'line', st.get('object-id'), zoom)
                            dr_element.lines.extend([dr_line])
                        if st.get('pattern-image'):
                            dr_line = LineRuleProto()
                            dr_line.width = 0
                            dr_line.color = 0
                            icon = mwm_encode_image(st, prefix='pattern')
                            dr_line.pathsym.name = icon[0]
                            dr_line.pathsym.step = float(st.get('pattern-spacing', 0)) - 16
                            dr_line.pathsym.offset = st.get('pattern-offset', 0)
                            dr_line.priority = get_drape_priority(cl, 'line', st.get('object-id'))
                            store_visibility(cl, 'line', st.get('object-id'), zoom)
                            dr_element.lines.extend([dr_line])

                        if st.get('shield-font-size'):
                            dr_element.shield.height = int(st.get('shield-font-size', 10))
                            dr_element.shield.text_color = mwm_encode_color(colors, st, "shield-text")
                            if st.get('shield-text-halo-radius', 0) != 0:
                                dr_element.shield.text_stroke_color = mwm_encode_color(colors, st, "shield-text-halo", "white")
                            dr_element.shield.color = mwm_encode_color(colors, st, "shield")
                            if st.get('shield-outline-radius', 0) != 0:
                                dr_element.shield.stroke_color = mwm_encode_color(colors, st, "shield-outline", "white")
                            dr_element.shield.priority = get_drape_priority(cl, 'shield', st.get('object-id'))
                            store_visibility(cl, 'shield', st.get('object-id'), zoom)
                            if st.get('shield-min-distance', 0) != 0:
                                dr_element.shield.min_distance = int(st.get('shield-min-distance', 0))

                    if has_icons:
                        if st.get('icon-image'):
                            icon = mwm_encode_image(st)
                            dr_element.symbol.name = icon[0]
                            dr_element.symbol.priority = get_drape_priority(cl, 'icon', st.get('object-id'))
                            store_visibility(cl, 'icon', st.get('object-id'), zoom)
                            if 'icon-min-distance' in st:
                                dr_element.symbol.min_distance = int(st.get('icon-min-distance', 0))
                            has_icons = False
                        if st.get('symbol-shape'):
                            # TODO: not used in current styles; do "circles" work in drape at all?
                            dr_element.circle.radius = float(st.get('symbol-size'))
                            dr_element.circle.color = mwm_encode_color(colors, st, 'symbol-fill')
                            dr_element.circle.priority = get_drape_priority(cl, 'circle', st.get('object-id'))
                            store_visibility(cl, 'circle', st.get('object-id'), zoom)
                            has_icons = False

                    if has_text and st.get('text') and st.get('text') != 'none':
                        # Take only first 2 captions: primary, secondary.
                        has_text = has_text[:2]

                        dr_text = dr_element.caption
                        text_priority_key = 'caption'
                        if st.get('text-position', 'center') == 'line':
                            dr_text = dr_element.path_text
                            text_priority_key = 'pathtext'

                        dr_cur_subtext = dr_text.primary
                        for sp in has_text:
                            dr_cur_subtext.height = int(float(sp.get('font-size', "10").split(",")[0]))
                            dr_cur_subtext.color = mwm_encode_color(colors, sp, "text")
                            if st.get('text-halo-radius', 0) != 0:
                                dr_cur_subtext.stroke_color = mwm_encode_color(colors, sp, "text-halo", "white")
                            if 'text-offset' in sp or 'text-offset-y' in sp:
                                dr_cur_subtext.offset_y = int(sp.get('text-offset-y', sp.get('text-offset', 0)))
                            if 'text-offset-x' in sp:
                                dr_cur_subtext.offset_x = int(sp.get('text-offset-x', 0))
                            if 'text' in sp and sp.get('text') != 'name':
                                dr_cur_subtext.text = sp.get('text')
                            if 'text-optional' in sp:
                                is_valid, value = to_boolean(sp.get('text-optional', ''))
                                if is_valid:
                                    dr_cur_subtext.is_optional = value
                                else:
                                    dr_cur_subtext.is_optional = True
                            elif text_priority_key == 'caption' and dr_element.symbol.priority:
                                # On by default for all captions (not path texts) with icons.
                                dr_cur_subtext.is_optional = True
                            dr_cur_subtext = dr_text.secondary

                        if text_priority_key == 'caption' and dr_element.symbol.priority:
                            # Mandatory captions with icons use icon's priority.
                            dr_text.priority = get_drape_priority(cl, 'icon', st.get('object-id'))
                            # Optional captions (with icons) are automatically placed below all other overlays.
                            if dr_text.primary.is_optional:
                                dr_text.priority -= OVERLAYS_MAX_PRIORITY
                        else:
                            dr_text.priority = get_drape_priority(cl, text_priority_key, st.get('object-id'))

                        store_visibility(cl, text_priority_key, st.get('object-id'), zoom)

                        # Process captions block once.
                        has_text = None

                    if has_fills:
                        if ('fill-color' in st) and (float(st.get('fill-opacity', 1)) > 0):
                            dr_element.area.color = mwm_encode_color(colors, st, "fill")
                            dr_element.area.priority = get_drape_priority(cl, 'area', st.get('object-id'))
                            store_visibility(cl, 'area', st.get('object-id'), zoom)
                            has_fills = False

                str_dr_element = dr_cont.name + "/" + str(dr_element)
                if str_dr_element not in all_draw_elements:
                    all_draw_elements.add(str_dr_element)
                    dr_cont.element.extend([dr_element])

    if dr_cont is not None:
        if dr_cont.element:
            drules.cont.extend([dr_cont])

        visibility["world|" + class_tree[cl] + "|"] = "".join(visstring)

    validate_visibilities(options.maxzoom)

    output = ''
    for prio_range in prio_ranges.keys():
        dump_priorities(prio_range, options.priorities_path, options.maxzoom)
        output += f'{"" if not output else ", "}{len(prio_ranges[prio_range]["priorities"])} {prio_range}'
    print(f'Re-formated priorities files: {output}.')

    # Write drules_proto.bin and drules_proto.txt files

    drules_bin = open(os.path.join(options.outfile + '.bin'), "wb")
    drules_bin.write(drules.SerializeToString())
    drules_bin.close()

    if options.txt:
        drules_txt = open(os.path.join(options.outfile + '.txt'), "wb")
        drules_txt.write(str(drules).encode())
        drules_txt.close()

    # Write classificator.txt and visibility.txt files

    visnodes = set()
    for k, v in visibility.items():
        vis = k.split("|")
        for i in range(1, len(vis) - 1):
            visnodes.add("|".join(vis[0:i]) + "|")
    viskeys = list(set(list(visibility.keys()) + list(visnodes)))

    def cmprepl(a, b):
        if a == b:
            return 0
        a = a.replace("|", "-")
        b = b.replace("|", "-")
        if a > b:
            return 1
        return -1
    viskeys.sort(key=functools.cmp_to_key(cmprepl))

    visibility_file = open(os.path.join(ddir, 'visibility.txt'), "w")
    classificator_file = open(os.path.join(ddir, 'classificator.txt'), "w")

    oldoffset = ""
    for k in viskeys:
        offset = "    " * (k.count("|") - 1)
        for i in range(int(len(oldoffset) / 4), int(len(offset) / 4), -1):
            print("    " * i + "{}", file=visibility_file)
            print("    " * i + "{}", file=classificator_file)
        oldoffset = offset
        end = "-"
        if k in visnodes:
            end = "+"
        print(offset + k.split("|")[-2] + "  " + visibility.get(k, "0" * (options.maxzoom + 1)) + "  " + end, file=visibility_file)
        print(offset + k.split("|")[-2] + "  " + end, file=classificator_file)
    for i in range(int(len(offset) / 4), 0, -1):
        print("    " * i + "{}", file=visibility_file)
        print("    " * i + "{}", file=classificator_file)

    visibility_file.close()
    classificator_file.close()

    colors_file = open(colors_file_name, "w")
    for c in sorted(colors):
        colors_file.write("%d\n" % (c))
    colors_file.close()

    patterns_file = open(patterns_file_name, "w")
    for p in patterns:
        patterns_file.write("%s\n" % (' '.join(str(elem) for elem in p)))
    patterns_file.close()


def main():
    parser = OptionParser()
    parser.add_option("-s", "--stylesheet", dest="filename",
                      help="read MapCSS stylesheet from FILE", metavar="FILE")
    parser.add_option("-f", "--minzoom", dest="minzoom", default=0, type="int",
                      help="minimal available zoom level", metavar="ZOOM")
    parser.add_option("-t", "--maxzoom", dest="maxzoom", default=19, type="int",
                      help="maximal available zoom level", metavar="ZOOM")
    parser.add_option("-o", "--output-file", dest="outfile", default="-",
                      help="output filename", metavar="FILE")
    parser.add_option("-x", "--txt", dest="txt", action="store_true",
                      help="create a text file for output", default=False)
    parser.add_option("-p", "--priorities-path", dest="priorities_path",
                      help="path to priorities *.prio.txt files", metavar="PATH")
    parser.add_option("-d", "--data-path", dest="data",
                      help="path to mapcss-mapping.csv and other files", metavar="PATH")

    (options, args) = parser.parse_args()

    if (options.filename is None):
        parser.error("MapCSS stylesheet filename is required")

    if options.outfile == "-":
        parser.error("Please specify base output path.")

    if (options.priorities_path is None or not os.path.isdir(options.priorities_path)):
        parser.error("A path to priorities *.prio.txt files is required.")
    options.priorities_path = os.path.normpath(options.priorities_path)

    komap_mapswithme(options)

if __name__ == '__main__':
    if PROFILE:
        import cProfile
        cProfile.run('main()', 'profile.tmp')
        import pstats
        p = pstats.Stats('profile.tmp')
        p.sort_stats('cumulative').print_stats(10)
    else:
        main()
