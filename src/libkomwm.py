from mapcss import MapCSS
from optparse import OptionParser
import os
import csv
import sys
import itertools
from multiprocessing import Pool
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
    cl, cltags, minzoom, maxzoom = args
    clname = cl if cl.find('-') == -1 else cl[:cl.find('-')]

    cltags["name"] = "name"
    cltags["addr:housenumber"] = "addr:housenumber"
    cltags["addr:housename"] = "addr:housename"
    cltags["ref"] = "ref"
    cltags["int_name"] = "int_name"
    cltags["addr:flats"] = "addr:flats"

    results = []
    for zoom in xrange(minzoom, maxzoom + 1):
        runtime_conditions_arr = []

        # Get runtime conditions which are used for class 'cl' on zoom 'zoom'
        if "area" not in cltags:
            runtime_conditions_arr.extend(style.get_runtime_rules(clname, "line", cltags, zoom))
        runtime_conditions_arr.extend(style.get_runtime_rules(clname, "area", cltags, zoom))
        if "area" not in cltags:
            runtime_conditions_arr.extend(style.get_runtime_rules(clname, "node", cltags, zoom))

        # If there is no any runtime conditions, do not filter style by runtime conditions
        if len(runtime_conditions_arr) == 0:
            runtime_conditions_arr.append(None)

        for runtime_conditions in runtime_conditions_arr:
            has_icons_for_areas = False
            zstyle = {}

            # Get style for class 'cl' on zoom 'zoom' with corresponding runtime conditions
            if "area" not in cltags:
                linestyle = style.get_style_dict(clname, "line", cltags, zoom, olddict=zstyle, filter_by_runtime_conditions=runtime_conditions)
                zstyle = linestyle
            areastyle = style.get_style_dict(clname, "area", cltags, zoom, olddict=zstyle, filter_by_runtime_conditions=runtime_conditions)
            for st in areastyle.values():
                if "icon-image" in st or 'symbol-shape' in st or 'symbol-image' in st:
                    has_icons_for_areas = True
                    break
            zstyle = areastyle
            if "area" not in cltags:
                nodestyle = style.get_style_dict(clname, "node", cltags, zoom, olddict=zstyle, filter_by_runtime_conditions=runtime_conditions)
                zstyle = nodestyle

            results.append((cl, zoom, has_icons_for_areas, runtime_conditions, zstyle.values()))
    return results


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

    # Mapcss-mapping format
    #
    # A CSV table mapping tags to types. Some types can be deemed obsolete, either completely or replaced with a different type.
    #
    # Example row: highway|bus_stop;[highway=bus_stop];;name;int_name;22; (mind the last semicolon!)
    # It contains:
    # - type name: "highway|bus_stop" ('|' is converted to '-' internally)
    # - mapcss selector for tags: "[highway=bus_stop]" (you can group selectors and use e.g. [oneway?])
    # - "x" for an obsolete type or an empty cell otherwise
    # - primary title tag (usually "name")
    # - secondary title tag (usually "int_name")
    # - type id, sequential starting from 1
    # - replacement type for an obsolete tag, if exists
    #
    # A shorter format for above example: highway|bus_stop;22;
    # It leaves only columns 1, 6 and 7. For obsolete types with no replacement put "x" into the last column.
    # Obviously it works only for simple types that are produced from tags replacing '=' with '|'.
    #
    # An example of type with replacement:
    # highway|unsurfaced|disused;[highway=unsurfaced][disused?];x;name;int_name;838;highway|unclassified

    cnt = 1
    unique_types_check = set()
    for row in csv.reader(open(os.path.join(ddir, 'mapcss-mapping.csv')), delimiter=';'):
        if len(row) <= 1:
            # Allow for empty lines and comments that do not contain ';' symbol
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
            print >> types_file, "mapswithme"
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
        classificator[cl] = kv
        if row[2] != "x":
            class_order.append(cl)
            unique_types_check.add(cl)
            print >> types_file, row[0]
        else:
            # compatibility mode
            if row[6]:
                print >> types_file, row[6]
            else:
                print >> types_file, "mapswithme"
        class_tree[cl] = row[0]
    class_order.sort()
    types_file.close()

    # Get all mapcss static tags which are used in mapcss-mapping.csv
    # This is a dict with main_tag flags (True = appears first in types)
    mapcss_static_tags = {}
    for v in classificator.values():
        for i, t in enumerate(v.keys()):
            mapcss_static_tags[t] = mapcss_static_tags.get(t, True) and i == 0

    # Get all mapcss dynamic tags from mapcss-dynamic.txt
    mapcss_dynamic_tags = set([line.rstrip() for line in open(os.path.join(ddir, 'mapcss-dynamic.txt'))])

    # Parse style mapcss
    global style
    style = MapCSS(options.minzoom, options.maxzoom + 1)
    style.parse(filename=options.filename, static_tags=mapcss_static_tags,
                dynamic_tags=mapcss_dynamic_tags)

    # Build optimization tree - class/type -> StyleChoosers
    for cl in class_order:
        clname = cl if cl.find('-') == -1 else cl[:cl.find('-')]
        cltags = classificator[cl]
        style.build_choosers_tree(clname, "line", cltags)
        style.build_choosers_tree(clname, "area", cltags)
        style.build_choosers_tree(clname, "node", cltags)
    style.restore_choosers_order("line")
    style.restore_choosers_order("area")
    style.restore_choosers_order("node")

    # Get colors section from style
    style_colors = {}
    raw_style_colors = style.get_colors()
    if raw_style_colors is not None:
        unique_style_colors = set()
        for k in raw_style_colors.keys():
            unique_style_colors.add(k[:k.rindex('-')])
        for k in unique_style_colors:
            style_colors[k] = mwm_encode_color(colors, raw_style_colors, k)

    visibility = {}

    bgpos = 0

    dr_linecaps = {'none': BUTTCAP, 'butt': BUTTCAP, 'round': ROUNDCAP}
    dr_linejoins = {'none': NOJOIN, 'bevel': BEVELJOIN, 'round': ROUNDJOIN}

    # Build drules tree

    drules = ContainerProto()
    dr_cont = None
    if MULTIPROCESSING:
        pool = Pool()
        imapfunc = pool.imap
    else:
        imapfunc = itertools.imap

    if style_colors:
        for k, v in style_colors.iteritems():
            color_proto = ColorElementProto()
            color_proto.name = k
            color_proto.color = v
            color_proto.x = 0
            color_proto.y = 0
            drules.colors.value.extend([color_proto])

    all_draw_elements = set()

    for results in imapfunc(query_style, ((cl, classificator[cl], options.minzoom, options.maxzoom) for cl in class_order)):
        for result in results:
                cl, zoom, has_icons_for_areas, runtime_conditions, zstyle = result

                if dr_cont is not None and dr_cont.name != cl:
                    if dr_cont.element:
                        drules.cont.extend([dr_cont])
                    visibility["world|" + class_tree[dr_cont.name] + "|"] = "".join(visstring)
                    dr_cont = None

                if dr_cont is None:
                    dr_cont = ClassifElementProto()
                    dr_cont.name = cl

                    visstring = ["0"] * (options.maxzoom - options.minzoom + 1)

                if len(zstyle) == 0:
                    continue

                has_lines = False
                has_icons = False
                has_fills = False
                for st in zstyle:
                    st = dict([(k, v) for k, v in st.iteritems() if str(v).strip(" 0.")])
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

                dr_element = DrawElementProto()
                dr_element.scale = zoom

                if runtime_conditions:
                    for rc in runtime_conditions:
                        dr_element.apply_if.append(str(rc))

                for st in zstyle:
                    if st.get('-x-kot-layer') == 'top':
                        st['z-index'] = float(st.get('z-index', 0)) + 15001.
                    elif st.get('-x-kot-layer') == 'bottom':
                        st['z-index'] = float(st.get('z-index', 0)) - 15001.

                    if st.get('casing-width') not in (None, 0):  # and (st.get('width') or st.get('fill-color')):
                        if has_lines and st.get('casing-linecap', 'butt') == 'butt':
                            dr_line = LineRuleProto()
                            dr_line.width = (st.get('width', 0) * WIDTH_SCALE) + (st.get('casing-width') * WIDTH_SCALE * 2)
                            dr_line.color = mwm_encode_color(colors, st, "casing")
                            if '-x-me-casing-line-priority' in st:
                                dr_line.priority = int(st.get('-x-me-casing-line-priority'))
                            else:
                                dr_line.priority = min(int(st.get('z-index', 0) + 999), 20000)
                            dashes = st.get('casing-dashes', st.get('dashes', []))
                            dr_line.dashdot.dd.extend(dashes)
                            addPattern(dr_line.dashdot.dd)
                            dr_line.cap = dr_linecaps.get(st.get('casing-linecap', 'butt'), BUTTCAP)
                            dr_line.join = dr_linejoins.get(st.get('casing-linejoin', 'round'), ROUNDJOIN)
                            dr_element.lines.extend([dr_line])

                        if has_fills and 'fill-color' in st and float(st.get('fill-opacity', 1)) > 0:
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
                            if '-x-me-line-priority' in st:
                                dr_line.priority = int(st.get('-x-me-line-priority'))
                            else:
                                dr_line.priority = min((int(st.get('z-index', 0)) + 1000), 20000)
                            dr_element.lines.extend([dr_line])
                        if st.get('pattern-image'):
                            dr_line = LineRuleProto()
                            dr_line.width = 0
                            dr_line.color = 0
                            icon = mwm_encode_image(st, prefix='pattern')
                            dr_line.pathsym.name = icon[0]
                            dr_line.pathsym.step = float(st.get('pattern-spacing', 0)) - 16
                            dr_line.pathsym.offset = st.get('pattern-offset', 0)
                            if '-x-me-line-priority' in st:
                                dr_line.priority = int(st.get('-x-me-line-priority'))
                            else:
                                dr_line.priority = int(st.get('z-index', 0)) + 1000
                            dr_element.lines.extend([dr_line])
                        if st.get('shield-font-size'):
                            dr_element.shield.height = int(st.get('shield-font-size', 10))
                            dr_element.shield.text_color = mwm_encode_color(colors, st, "shield-text")
                            if st.get('shield-text-halo-radius', 0) != 0:
                                dr_element.shield.text_stroke_color = mwm_encode_color(colors, st, "shield-text-halo", "white")
                            dr_element.shield.color = mwm_encode_color(colors, st, "shield")
                            if st.get('shield-outline-radius', 0) != 0:
                                dr_element.shield.stroke_color = mwm_encode_color(colors, st, "shield-outline", "white")
                            if '-x-me-shield-priority' in st:
                                dr_element.shield.priority = int(st.get('-x-me-shield-priority'))
                            else:
                                dr_element.shield.priority = min(19100, (16000 + int(st.get('z-index', 0))))
                            if st.get('shield-min-distance', 0) != 0:
                                dr_element.shield.min_distance = int(st.get('shield-min-distance', 0))

                    if has_icons:
                        if st.get('icon-image'):
                            if not has_icons_for_areas:
                                dr_element.symbol.apply_for_type = 1
                            icon = mwm_encode_image(st)
                            dr_element.symbol.name = icon[0]
                            if '-x-me-icon-priority' in st:
                                dr_element.symbol.priority = int(st.get('-x-me-icon-priority'))
                            else:
                                dr_element.symbol.priority = min(19100, (16000 + int(st.get('z-index', 0))))
                            if 'icon-min-distance' in st:
                                dr_element.symbol.min_distance = int(st.get('icon-min-distance', 0))
                            has_icons = False
                        if st.get('symbol-shape'):
                            dr_element.circle.radius = float(st.get('symbol-size'))
                            dr_element.circle.color = mwm_encode_color(colors, st, 'symbol-fill')
                            if '-x-me-symbol-priority' in st:
                                dr_element.circle.priority = int(st.get('-x-me-symbol-priority'))
                            else:
                                dr_element.circle.priority = min(19000, (14000 + int(st.get('z-index', 0))))
                            has_icons = False

                    if has_text and st.get('text') and st.get('text') != 'none':
                        has_text = has_text[:2]
                        has_text.reverse()
                        dr_text = dr_element.path_text
                        base_z = 15000
                        if st.get('text-position', 'center') == 'line':
                            dr_text = dr_element.path_text
                            base_z = 16000
                        else:
                            dr_text = dr_element.caption
                        for sp in has_text[:]:
                            dr_cur_subtext = dr_text.primary
                            if len(has_text) == 2:
                                dr_cur_subtext = dr_text.secondary
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
                            has_text.pop()
                        if '-x-me-text-priority' in st:
                            dr_text.priority = int(st.get('-x-me-text-priority'))
                        else:
                            dr_text.priority = min(19000, (base_z + int(st.get('z-index', 0))))
                        has_text = None

                    if has_fills:
                        if ('fill-color' in st) and (float(st.get('fill-opacity', 1)) > 0):
                            dr_element.area.color = mwm_encode_color(colors, st, "fill")
                            priority = 0
                            if st.get('fill-position', 'foreground') == 'background':
                                if 'z-index' not in st:
                                    bgpos -= 1
                                    priority = bgpos - 16000
                                else:
                                    zzz = int(st.get('z-index', 0))
                                    if zzz > 0:
                                        priority = zzz - 16000
                                    else:
                                        priority = zzz - 16700
                            else:
                                priority = (int(st.get('z-index', 0)) + 1 + 1000)
                            if '-x-me-area-priority' in st:
                                dr_element.area.priority = int(st.get('-x-me-area-priority'))
                            else:
                                dr_element.area.priority = priority
                            has_fills = False

                str_dr_element = dr_cont.name + "/" + str(dr_element)
                if str_dr_element not in all_draw_elements:
                    all_draw_elements.add(str_dr_element)
                    dr_cont.element.extend([dr_element])

    if dr_cont is not None:
        if dr_cont.element:
            drules.cont.extend([dr_cont])

        visibility["world|" + class_tree[cl] + "|"] = "".join(visstring)

    # Write drules_proto.bin and drules_proto.txt files

    drules_bin = open(os.path.join(options.outfile + '.bin'), "wb")
    drules_bin.write(drules.SerializeToString())
    drules_bin.close()

    if options.txt:
        drules_txt = open(os.path.join(options.outfile + '.txt'), "wb")
        drules_txt.write(unicode(drules))
        drules_txt.close()

    # Write classificator.txt and visibility.txt files

    visnodes = set()
    for k, v in visibility.iteritems():
        vis = k.split("|")
        for i in range(1, len(vis) - 1):
            visnodes.add("|".join(vis[0:i]) + "|")
    viskeys = list(set(visibility.keys() + list(visnodes)))

    def cmprepl(a, b):
        if a == b:
            return 0
        a = a.replace("|", "-")
        b = b.replace("|", "-")
        if a > b:
            return 1
        return -1
    viskeys.sort(cmprepl)

    visibility_file = open(os.path.join(ddir, 'visibility.txt'), "w")
    classificator_file = open(os.path.join(ddir, 'classificator.txt'), "w")

    oldoffset = ""
    for k in viskeys:
        offset = "    " * (k.count("|") - 1)
        for i in range(len(oldoffset) / 4, len(offset) / 4, -1):
            print >> visibility_file, "    " * i + "{}"
            print >> classificator_file, "    " * i + "{}"
        oldoffset = offset
        end = "-"
        if k in visnodes:
            end = "+"
        print >> visibility_file, offset + k.split("|")[-2] + "  " + visibility.get(k, "0" * (options.maxzoom + 1)) + "  " + end
        print >> classificator_file, offset + k.split("|")[-2] + "  " + end
    for i in range(len(offset) / 4, 0, -1):
        print >> visibility_file, "    " * i + "{}"
        print >> classificator_file, "    " * i + "{}"

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
    parser.add_option("-d", "--data-path", dest="data",
                      help="path to mapcss-mapping.csv and other files", metavar="PATH")

    (options, args) = parser.parse_args()

    if (options.filename is None):
        parser.error("MapCSS stylesheet filename is required")

    if options.outfile == "-":
        parser.error("Please specify base output path.")

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
