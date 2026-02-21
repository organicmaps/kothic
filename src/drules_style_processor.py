"""
Style processing and drawing rule generation.

This module works entirely with Python dictionaries. No protobuf dependencies.
"""
import logging
from typing import List, Set, Dict, Any, Callable, Optional
from drules_utils import to_boolean, mwm_encode_color, mwm_encode_image
from drules_config import OVERLAYS_MAX_PRIORITY

# Configure module logger
logger = logging.getLogger(__name__)


class StyleProcessor:
    """
    Processes MapCSS styles and generates drawing rules as dictionaries.

    All methods return plain Python dictionaries. Protobuf conversion
    happens only in the serialization layer.
    """

    def __init__(self, priority_manager: Any, visibility_tracker: Any,
                 colors: Set[int], add_pattern_func: Callable[[List[float]], None]) -> None:
        """
        Initialize style processor.

        Args:
            priority_manager: PriorityManager instance
            visibility_tracker: VisibilityTracker instance
            colors: Set to add colors to
            add_pattern_func: Function to add patterns
        """
        self.priority_manager = priority_manager
        self.visibility_tracker = visibility_tracker
        self.colors = colors
        self.add_pattern = add_pattern_func
        self.validation_errors_count = 0

        # Drawing style constants - stored as strings for format independence
        self.dr_linecaps: Dict[str, str] = {
            'none': 'butt',
            'butt': 'butt',
            'round': 'round'
        }
        self.dr_linejoins: Dict[str, str] = {
            'none': 'none',
            'bevel': 'bevel',
            'round': 'round'
        }

    def process_casing(self, st: Dict[str, Any], zstyle: List[Dict[str, Any]],
                      has_lines: bool, has_fills: bool, zoom: int, cl: str) -> List[Dict[str, Any]]:
        """
        Process casing rules for lines and areas.

        Returns:
            List of line rule dictionaries
        """
        line_rules = []

        if st.get('casing-width') not in (None, 0) or st.get('casing-width-add') is not None:
            is_area_st = 'fill-color' in st

            if has_lines and not is_area_st and st.get('casing-linecap', 'butt') == 'butt':
                base_width = st.get('width', 0)
                if base_width == 0:
                    for wst in zstyle:
                        if wst.get('width') not in (None, 0):
                            if base_width == 0 or wst.get('object-id') != '::default':
                                base_width = wst.get('width', 0)
                    if st.get('casing-width') in (None, 0):
                        st['casing-width'] = base_width + st.get('casing-width-add')
                        base_width = 0

                # Create line rule dictionary
                dr_line = {
                    'width': round(base_width + st.get('casing-width') * 2, 2),
                    'color': mwm_encode_color(self.colors, st, "casing"),
                    'cap': self.dr_linecaps.get(st.get('casing-linecap', 'butt'), 'butt'),
                    'join': self.dr_linejoins.get(st.get('casing-linejoin', 'round'), 'round'),
                    'dashdot': None,
                    'pathsym': None
                }

                if st.get('object-id') == '::default':
                    auto_comment = 'casing'
                    dr_line['priority'] = self.priority_manager.get_drape_priority(
                        cl, 'line', st.get('object-id'), 'line', auto_comment, -1)
                    self.visibility_tracker.store_visibility(cl, 'line', st.get('object-id'), zoom, auto_comment)
                else:
                    dr_line['priority'] = self.priority_manager.get_drape_priority(cl, 'line', st.get('object-id'))
                    self.visibility_tracker.store_visibility(cl, 'line', st.get('object-id'), zoom)

                dashes = st.get('casing-dashes', st.get('dashes', []))
                if dashes:
                    dr_line['dashdot'] = {
                        'dd': [float(i) for i in dashes],
                        'offset': 0.0
                    }
                    self.add_pattern(dr_line['dashdot']['dd'])

                line_rules.append(dr_line)

        return line_rules

    def process_line_rules(self, st: Dict[str, Any], zoom: int, cl: str) -> List[Dict[str, Any]]:
        """Process line drawing rules. Returns list of line rule dictionaries."""
        line_rules = []

        if st.get('width'):
            dashes = st.get('dashes', [])
            dr_line = {
                'width': st.get('width', 0),
                'color': mwm_encode_color(self.colors, st),
                'cap': self.dr_linecaps.get(st.get('linecap', 'butt'), 'butt'),
                'join': self.dr_linejoins.get(st.get('linejoin', 'round'), 'round'),
                'priority': self.priority_manager.get_drape_priority(cl, 'line', st.get('object-id')),
                'dashdot': None,
                'pathsym': None
            }
            if dashes:
                dr_line['dashdot'] = {
                    'dd': [float(i) for i in dashes],
                    'offset': 0.0
                }
                self.add_pattern(dr_line['dashdot']['dd'])

            self.visibility_tracker.store_visibility(cl, 'line', st.get('object-id'), zoom)
            line_rules.append(dr_line)

        if st.get('pattern-image'):
            icon = mwm_encode_image(st, prefix='pattern')
            if icon:
                # Extract pattern properties for pathsym
                # step = pattern-spacing - 16 (magic constant for arrow spacing)
                # offset = pattern-offset (direct)
                pattern_spacing = float(st.get('pattern-spacing', 0))
                pattern_offset = float(st.get('pattern-offset', 0))
                step = pattern_spacing - 16.0 if pattern_spacing > 0 else 0.0
                offset = pattern_offset

                dr_line = {
                    'width': 0,
                    'color': 0,
                    'cap': 'butt',
                    'join': 'round',
                    'priority': self.priority_manager.get_drape_priority(cl, 'line', st.get('object-id')),
                    'dashdot': None,
                    'pathsym': {
                        'name': icon,
                        'step': step,
                        'offset': offset
                    }
                }
                self.visibility_tracker.store_visibility(cl, 'line', st.get('object-id'), zoom)
                line_rules.append(dr_line)

        return line_rules

    def process_shield_rule(self, st: Dict[str, Any], zoom: int, cl: str, dr_element: Dict[str, Any]) -> None:
        """Process shield drawing rule. Updates dr_element dictionary."""
        if st.get('shield-font-size'):
            dr_element['shield'] = {
                'height': int(st.get('shield-font-size', 10)),
                'text_color': mwm_encode_color(self.colors, st, "shield-text"),
                'text_stroke_color': 0,
                'color': mwm_encode_color(self.colors, st, "shield"),
                'stroke_color': 0,
                'priority': self.priority_manager.get_drape_priority(cl, 'shield', st.get('object-id')),
                'min_distance': 0
            }

            if st.get('shield-text-halo-radius', 0) != 0:
                dr_element['shield']['text_stroke_color'] = mwm_encode_color(self.colors, st, "shield-text-halo", "white")
            if st.get('shield-outline-radius', 0) != 0:
                dr_element['shield']['stroke_color'] = mwm_encode_color(self.colors, st, "shield-outline", "white")
            if st.get('shield-min-distance', 0) != 0:
                dr_element['shield']['min_distance'] = int(st.get('shield-min-distance', 0))

            self.visibility_tracker.store_visibility(cl, 'shield', st.get('object-id'), zoom)

    def process_icon_and_circle(self, st: Dict[str, Any], zoom: int, cl: str,
                                dr_element: Dict[str, Any], has_icons: bool) -> bool:
        """Process icon and circle drawing rules. Updates dr_element dictionary."""
        if has_icons:
            if st.get('icon-image') and st.get('icon-image') != 'none':
                icon = mwm_encode_image(st)
                if icon:
                    dr_element['symbol'] = {
                        'name': icon,
                        'apply_for_type': 0,
                        'priority': self.priority_manager.get_drape_priority(cl, 'icon', st.get('object-id')),
                        'min_distance': int(st.get('icon-min-distance', 0)) if 'icon-min-distance' in st else 0
                    }
                    self.visibility_tracker.store_visibility(cl, 'icon', st.get('object-id'), zoom)
                    has_icons = False

            if st.get('symbol-shape'):
                dr_element['circle'] = {
                    'radius': float(st.get('symbol-size')),
                    'color': mwm_encode_color(self.colors, st, 'symbol-fill'),
                    'priority': self.priority_manager.get_drape_priority(cl, 'circle', st.get('object-id')),
                    'border': None
                }
                self.visibility_tracker.store_visibility(cl, 'circle', st.get('object-id'), zoom)
                has_icons = False

        return has_icons

    def process_text_rules(self, st: Dict[str, Any], has_text: Optional[List[Dict[str, Any]]],
                           zoom: int, cl: str, dr_element: Dict[str, Any]) -> Optional[List[Dict[str, Any]]]:
        """Process text/caption drawing rules. Updates dr_element dictionary."""
        if has_text and st.get('text') and st.get('text') != 'none':
            has_text = has_text[:2]  # Take only first 2 captions

            text_priority_key = 'caption'
            if st.get('text-position', 'center') == 'line':
                text_priority_key = 'pathtext'

            # Build caption/path_text rule
            primary_caption = None
            secondary_caption = None

            for i, sp in enumerate(has_text):
                caption_def = {
                    'height': int(float(sp.get('font-size', "10").split(",")[0])),
                    'color': 0,
                    'stroke_color': 0,
                    'offset_x': 0,
                    'offset_y': 0,
                    'text': '',
                    'is_optional': False
                }

                if 'text-color' not in st:
                    logger.error(f'text-color not set for z{zoom} {cl}')
                    self.validation_errors_count += 1
                else:
                    caption_def['color'] = mwm_encode_color(self.colors, sp, "text")

                if st.get('text-halo-radius', 0) != 0:
                    caption_def['stroke_color'] = mwm_encode_color(self.colors, sp, "text-halo", "white")

                if 'text-offset' in sp or 'text-offset-y' in sp:
                    caption_def['offset_y'] = int(sp.get('text-offset-y', sp.get('text-offset', 0)))
                elif 'text-offset-x' in sp:
                    caption_def['offset_x'] = int(sp.get('text-offset-x', 0))
                elif st.get('text-position', 'center') == 'center' and dr_element.get('symbol'):
                    logger.error(f'an icon is present, but caption\'s text-offset is not set for z{zoom} {cl}')
                    self.validation_errors_count += 1

                if 'text' in sp and sp.get('text') not in ('name', 'int_name'):
                    caption_def['text'] = sp.get('text')

                if 'text-optional' in sp:
                    value = to_boolean(sp.get('text-optional', ''))
                    caption_def['is_optional'] = value if value is not None else True
                elif text_priority_key == 'caption' and dr_element.get('symbol'):
                    caption_def['is_optional'] = True

                if i == 0:
                    primary_caption = caption_def
                else:
                    secondary_caption = caption_def

            # Determine priority
            auto_comment = None
            priority = 0
            if text_priority_key == 'caption' and dr_element.get('symbol'):
                auto_prio_mod = 0
                auto_comment = 'mandatory'
                if primary_caption and primary_caption['is_optional']:
                    auto_comment = 'optional'
                    auto_prio_mod = -OVERLAYS_MAX_PRIORITY
                priority = self.priority_manager.get_drape_priority(
                    cl, 'icon', st.get('object-id'), text_priority_key, auto_comment, auto_prio_mod)
            else:
                priority = self.priority_manager.get_drape_priority(cl, text_priority_key, st.get('object-id'))

            # Add to dr_element
            text_rule = {
                'primary': primary_caption,
                'secondary': secondary_caption,
                'priority': priority
            }

            if text_priority_key == 'caption':
                dr_element['caption'] = text_rule
            else:
                dr_element['path_text'] = text_rule

            self.visibility_tracker.store_visibility(cl, text_priority_key, st.get('object-id'), zoom, auto_comment)
            has_text = None

        return has_text

    def process_area_rule(self, st: Dict[str, Any], zoom: int, cl: str,
                         dr_element: Dict[str, Any], has_fills: bool) -> bool:
        """Process area fill drawing rule. Updates dr_element dictionary."""
        if has_fills:
            if 'fill-color' in st and st.get('fill-color') != 'none' and float(st.get('fill-opacity', 1)) > 0:
                # Preserve existing border if it was set by casing processing
                existing_border = None
                if dr_element['area'] is not None:
                    existing_border = dr_element['area'].get('border')

                dr_element['area'] = {
                    'color': mwm_encode_color(self.colors, st, "fill"),
                    'priority': self.priority_manager.get_drape_priority(cl, 'area', st.get('object-id')),
                    'border': existing_border
                }
                self.visibility_tracker.store_visibility(cl, 'area', st.get('object-id'), zoom)
                has_fills = False

        return has_fills

