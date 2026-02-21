"""
Drules serialization module.

This module handles all protobuf serialization logic. To switch to FlatBuffers,
only this file needs to be modified - the rest of the codebase works with
plain Python dictionaries.
"""
import logging
from typing import Dict, List, Any, Set, Optional
from drules_struct_pb2 import (
    ContainerProto, ClassifElementProto, DrawElementProto,
    ColorElementProto, ColorsElementProto,
    LineRuleProto, AreaRuleProto, SymbolRuleProto,
    CaptionRuleProto, CircleRuleProto, PathTextRuleProto,
    ShieldRuleProto, CaptionDefProto, LineDefProto,
    DashDotProto, PathSymProto,
    LineCap, LineJoin
)

logger = logging.getLogger(__name__)


class DrulesSerializer:
    """
    Handles serialization of drules data to protobuf format.

    To switch to FlatBuffers:
    1. Replace imports with FlatBuffers generated code
    2. Update the build_* methods to use FlatBuffers API
    3. Update serialize() to return FlatBuffers byte array

    The data structure (dictionaries) remains the same.
    """

    def __init__(self):
        """Initialize the serializer."""
        self.linecap_map = {
            'none': LineCap.BUTTCAP,
            'butt': LineCap.BUTTCAP,
            'round': LineCap.ROUNDCAP
        }
        self.linejoin_map = {
            'none': LineJoin.NOJOIN,
            'bevel': LineJoin.BEVELJOIN,
            'round': LineJoin.ROUNDJOIN
        }

    def _build_dashdot(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build DashDotProto from dictionary."""
        if not data or not data.get('dd'):
            return None

        dd_proto = DashDotProto()
        for value in data['dd']:
            dd_proto.dd.extend([float(value)])
        if data.get('offset', 0.0) != 0.0:
            dd_proto.offset = data['offset']
        return dd_proto

    def _build_pathsym(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build PathSymProto from dictionary."""
        if not data or not data.get('name'):
            return None

        ps_proto = PathSymProto()
        ps_proto.name = data['name']
        if data.get('step', 0.0) != 0.0:
            ps_proto.step = data['step']
        if data.get('offset', 0.0) != 0.0:
            ps_proto.offset = data['offset']
        return ps_proto

    def _build_linedef(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build LineDefProto from dictionary (used for area borders)."""
        if not data:
            return None

        ld_proto = LineDefProto()
        if data.get('width', 0.0) != 0.0:
            ld_proto.width = data['width']
        if data.get('color', 0) != 0:
            ld_proto.color = data['color']

        dashdot = self._build_dashdot(data.get('dashdot'))
        if dashdot:
            ld_proto.dashdot.CopyFrom(dashdot)

        # NOTE: LineDefProto (for area borders) does NOT have cap/join fields
        # Only LineRuleProto (for lines) has those
        # The original code only sets width and color for area.border

        return ld_proto

    def _build_linerule(self, data: Dict[str, Any]) -> Any:
        """Build LineRuleProto from dictionary."""
        lr_proto = LineRuleProto()

        if data.get('width', 0.0) != 0.0:
            lr_proto.width = data['width']
        if data.get('color', 0) != 0:
            lr_proto.color = data['color']
        if data.get('priority', 0) != 0:
            lr_proto.priority = data['priority']

        dashdot = self._build_dashdot(data.get('dashdot'))
        if dashdot:
            lr_proto.dashdot.CopyFrom(dashdot)

        pathsym = self._build_pathsym(data.get('pathsym'))
        if pathsym:
            lr_proto.pathsym.CopyFrom(pathsym)

        # Only set cap and join for lines with width (not for pattern symbol lines)
        # Original code only sets these when st.get('width') exists
        if data.get('width', 0.0) != 0.0:
            lr_proto.cap = self.linecap_map.get(data.get('cap', 'butt'), LineCap.BUTTCAP)
            lr_proto.join = self.linejoin_map.get(data.get('join', 'round'), LineJoin.ROUNDJOIN)

        return lr_proto

    def _build_arearule(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build AreaRuleProto from dictionary."""
        if not data:
            return None

        # In protobuf, area can have just a border without fill color
        # So we check if either color is non-zero OR border exists
        has_color = data.get('color', 0) != 0
        has_border = data.get('border') is not None

        if not has_color and not has_border:
            return None

        ar_proto = AreaRuleProto()
        if data.get('color', 0) != 0:
            ar_proto.color = data['color']
        if data.get('priority', 0) != 0:
            ar_proto.priority = data['priority']

        border = self._build_linedef(data.get('border'))
        if border:
            ar_proto.border.CopyFrom(border)

        return ar_proto

    def _build_captiondef(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build CaptionDefProto from dictionary."""
        if not data:
            return None

        cd_proto = CaptionDefProto()

        if data.get('height', 0) != 0:
            cd_proto.height = data['height']
        if data.get('color', 0) != 0:
            cd_proto.color = data['color']
        if data.get('stroke_color', 0) != 0:
            cd_proto.stroke_color = data['stroke_color']
        if data.get('offset_x', 0) != 0:
            cd_proto.offset_x = data['offset_x']
        if data.get('offset_y', 0) != 0:
            cd_proto.offset_y = data['offset_y']
        if data.get('text'):
            cd_proto.text = data['text']
        if data.get('is_optional', False):
            cd_proto.is_optional = data['is_optional']

        return cd_proto

    def _build_captionrule(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build CaptionRuleProto from dictionary."""
        if not data:
            return None

        cr_proto = CaptionRuleProto()

        primary = self._build_captiondef(data.get('primary'))
        if primary:
            cr_proto.primary.CopyFrom(primary)

        secondary = self._build_captiondef(data.get('secondary'))
        if secondary:
            cr_proto.secondary.CopyFrom(secondary)

        if data.get('priority', 0) != 0:
            cr_proto.priority = data['priority']

        return cr_proto

    def _build_symbolrule(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build SymbolRuleProto from dictionary."""
        if not data or not data.get('name'):
            return None

        sr_proto = SymbolRuleProto()
        sr_proto.name = data['name']

        if data.get('apply_for_type', 0) != 0:
            sr_proto.apply_for_type = data['apply_for_type']
        if data.get('priority', 0) != 0:
            sr_proto.priority = data['priority']
        if data.get('min_distance', 0) != 0:
            sr_proto.min_distance = data['min_distance']

        return sr_proto

    def _build_circlerule(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build CircleRuleProto from dictionary."""
        if not data:
            return None

        circ_proto = CircleRuleProto()

        if data.get('radius', 0.0) != 0.0:
            circ_proto.radius = data['radius']
        if data.get('color', 0) != 0:
            circ_proto.color = data['color']
        if data.get('priority', 0) != 0:
            circ_proto.priority = data['priority']

        border = self._build_linedef(data.get('border'))
        if border:
            circ_proto.border.CopyFrom(border)

        return circ_proto

    def _build_pathtextrule(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build PathTextRuleProto from dictionary."""
        if not data:
            return None

        ptr_proto = PathTextRuleProto()

        primary = self._build_captiondef(data.get('primary'))
        if primary:
            ptr_proto.primary.CopyFrom(primary)

        secondary = self._build_captiondef(data.get('secondary'))
        if secondary:
            ptr_proto.secondary.CopyFrom(secondary)

        if data.get('priority', 0) != 0:
            ptr_proto.priority = data['priority']

        return ptr_proto

    def _build_shieldrule(self, data: Optional[Dict[str, Any]]) -> Optional[Any]:
        """Build ShieldRuleProto from dictionary."""
        if not data:
            return None

        sh_proto = ShieldRuleProto()

        if data.get('height', 0) != 0:
            sh_proto.height = data['height']
        if data.get('color', 0) != 0:
            sh_proto.color = data['color']
        if data.get('stroke_color', 0) != 0:
            sh_proto.stroke_color = data['stroke_color']
        if data.get('priority', 0) != 0:
            sh_proto.priority = data['priority']
        if data.get('min_distance', 0) != 0:
            sh_proto.min_distance = data['min_distance']
        if data.get('text_color', 0) != 0:
            sh_proto.text_color = data['text_color']
        if data.get('text_stroke_color', 0) != 0:
            sh_proto.text_stroke_color = data['text_stroke_color']

        return sh_proto

    def _build_draw_element(self, data: Dict[str, Any]) -> Any:
        """Build DrawElementProto from dictionary."""
        de_proto = DrawElementProto()
        de_proto.scale = data['scale']

        # Apply-if conditions
        for condition in data.get('apply_if', []):
            de_proto.apply_if.append(condition)

        # Lines
        for line_data in data.get('lines', []):
            line_proto = self._build_linerule(line_data)
            de_proto.lines.extend([line_proto])

        # Area
        area = self._build_arearule(data.get('area'))
        if area:
            de_proto.area.CopyFrom(area)

        # Symbol
        symbol = self._build_symbolrule(data.get('symbol'))
        if symbol:
            de_proto.symbol.CopyFrom(symbol)

        # Caption
        caption = self._build_captionrule(data.get('caption'))
        if caption:
            de_proto.caption.CopyFrom(caption)

        # Circle
        circle = self._build_circlerule(data.get('circle'))
        if circle:
            de_proto.circle.CopyFrom(circle)

        # Path text
        path_text = self._build_pathtextrule(data.get('path_text'))
        if path_text:
            de_proto.path_text.CopyFrom(path_text)

        # Shield
        shield = self._build_shieldrule(data.get('shield'))
        if shield:
            de_proto.shield.CopyFrom(shield)

        return de_proto

    def _build_classif_element(self, data: Dict[str, Any]) -> Any:
        """Build ClassifElementProto from dictionary."""
        ce_proto = ClassifElementProto()
        ce_proto.name = data['name']

        for elem_data in data.get('elements', []):
            elem_proto = self._build_draw_element(elem_data)
            ce_proto.element.extend([elem_proto])

        return ce_proto

    def serialize(self, drules_data: Dict[str, Any]) -> bytes:
        """
        Serialize drules data to protobuf binary format.

        Args:
            drules_data: Dictionary containing:
                - 'colors': List of color dictionaries
                - 'classifications': List of classification dictionaries

        Returns:
            Binary protobuf data
        """
        logger.debug("Serializing drules data to protobuf format")

        container = ContainerProto()

        # Add colors
        if drules_data.get('colors'):
            for color_data in drules_data['colors']:
                color_proto = ColorElementProto()
                color_proto.name = color_data['name']
                color_proto.color = color_data['color']
                color_proto.x = color_data.get('x', 0)
                color_proto.y = color_data.get('y', 0)
                container.colors.value.extend([color_proto])

        # Add classifications
        for classif_data in drules_data.get('classifications', []):
            classif_proto = self._build_classif_element(classif_data)
            container.cont.extend([classif_proto])

        logger.debug(f"Serialized {len(drules_data.get('classifications', []))} classifications")

        return container.SerializeToString()

    def serialize_to_text(self, drules_data: Dict[str, Any]) -> str:
        """
        Serialize drules data to text format for debugging.

        Args:
            drules_data: Dictionary containing drules data

        Returns:
            Text representation
        """
        # First serialize to protobuf, then convert to string
        container = ContainerProto()

        # Add colors
        if drules_data.get('colors'):
            for color_data in drules_data['colors']:
                color_proto = ColorElementProto()
                color_proto.name = color_data['name']
                color_proto.color = color_data['color']
                color_proto.x = color_data.get('x', 0)
                color_proto.y = color_data.get('y', 0)
                container.colors.value.extend([color_proto])

        # Add classifications
        for classif_data in drules_data.get('classifications', []):
            classif_proto = self._build_classif_element(classif_data)
            container.cont.extend([classif_proto])

        return str(container)

