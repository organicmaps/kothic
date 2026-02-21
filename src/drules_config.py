"""
Configuration constants for drules generation.

This module defines priority ranges for drawing rules and their associated metadata.
Priority ranges control the rendering order of map features.
"""
import logging
from typing import Dict, Any, NamedTuple

# Configure module logger
logger = logging.getLogger(__name__)

# =============================================================================
# CONSTANTS
# =============================================================================

# Priority range for area and line drules. Should be same as drule::kLayerPriorityRange.
LAYER_PRIORITY_RANGE = 1000

# Should be same as drule::kOverlaysMaxPriority. The overlays range is [-kOverlaysMaxPriority; kOverlaysMaxPriority),
# negative values are used for optional captions which are below most other overlays.
OVERLAYS_MAX_PRIORITY = 10000

# Priority range identifiers
PRIO_OVERLAYS = 'overlays'
PRIO_FG = 'FG'
PRIO_BG_TOP = 'BG-top'
PRIO_BG_BY_SIZE = 'BG-by-size'

# =============================================================================
# FILE HEADER COMMENTS
# =============================================================================

COMMENT_AUTOFORMAT = (
    'This file is automatically re-formatted and re-sorted in priorities descending order\n'
    'when generate_drules.sh is run. All comments (automatic priorities of e.g. optional captions, drule types visibilities, etc.)\n'
    'are generated automatically for information only. Custom formatting and comments are not preserved.\n'
)

COMMENT_RANGES_OVERVIEW = (
    "\nPriorities ranges' rendering order overview:\n"
    '- overlays (icons, captions...)\n'
    '- FG: foreground areas and lines\n'
    '- BG-top: water (linear and areal)\n'
    '- BG-by-size: landcover areas sorted by their size\n'
)

# =============================================================================
# PRIORITY RANGE DESCRIPTIONS
# =============================================================================

COMMENT_OVERLAYS = (
    '\nOverlays (icons, captions, path texts and shields) are rendered on top of all the geometry (lines, areas).\n'
    "Overlays don't overlap each other, instead the ones with higher priority displace the less important ones.\n"
    'Optional captions (which have an icon) are usually displayed only if there are no other overlays in their way\n'
    f'(technically, max overlays priority value ({OVERLAYS_MAX_PRIORITY}) is subtracted from their priorities automatically).\n'
)

COMMENT_FG = (
    '\nFG geometry: foreground lines and areas (e.g. buildings) are rendered always below overlays\n'
    'and always on top of background geometry (BG-top & BG-by-size) even if a foreground feature\n'
    'is layer=-10 (as tunnels should be visibile over landcover and water).\n'
)

COMMENT_BG_TOP = (
    '\nBG-top geometry: background lines and areas that should be always below foreground ones\n'
    '(including e.g. layer=-10 underwater tunnels), but above background areas sorted by size (BG-by-size),\n'
    "because ordering by size doesn't always work with e.g. water mapped over a forest,\n"
    'so water should be on top of other landcover always, but linear waterways should be hidden beneath it.\n'
    'Still, e.g. a layer=-1 BG-top feature will be rendered under a layer=0 BG-by-size feature\n'
    '(so areal water tunnels are hidden beneath other landcover area) and a layer=1 landcover areas\n'
    'are displayed above layer=0 BG-top.\n'
)

COMMENT_BG_BY_SIZE = (
    '\nBG-by-size geometry: background areas rendered below BG-top and everything else.\n'
    "Smaller areas are rendered above larger ones (area's size is estimated as the size of its' bounding box).\n"
    'So effectively priority values of BG-by-size areas are not used at the moment.\n'
    'But we might use them later for some special cases, e.g. to determine a main area type of a multi-type feature.\n'
    'Keep them in a logical importance order please.\n'
)

# =============================================================================
# PRIORITY RANGE CONFIGURATION
# =============================================================================


class PriorityRangeConfig(NamedTuple):
    """Configuration for a priority range."""
    pos: int          # Rendering position (higher = rendered later/on top)
    base: int         # Base priority offset
    comment: str      # Description text for the priority file


# Priority range configurations - centralized and easy to maintain
PRIORITY_RANGE_CONFIGS = {
    PRIO_OVERLAYS: PriorityRangeConfig(
        pos=4,
        base=0,
        comment=COMMENT_OVERLAYS
    ),
    PRIO_FG: PriorityRangeConfig(
        pos=3,
        base=0,
        comment=COMMENT_FG
    ),
    PRIO_BG_TOP: PriorityRangeConfig(
        pos=2,
        base=-1000,
        comment=COMMENT_BG_TOP
    ),
    PRIO_BG_BY_SIZE: PriorityRangeConfig(
        pos=1,
        base=-2000,
        comment=COMMENT_BG_BY_SIZE
    ),
}


def get_prio_ranges() -> Dict[str, Dict[str, Any]]:
    """
    Initialize and return priority ranges with their configurations.

    Returns:
        Dictionary mapping priority range names to their configuration dicts.
        Each configuration contains:
        - pos: Rendering position (higher = on top)
        - base: Base priority offset
        - comment: Description for priority file
        - priorities: Dictionary of (type, object_id) -> priority mappings (empty initially)
    """
    prio_ranges = {}

    for range_name, config in PRIORITY_RANGE_CONFIGS.items():
        prio_ranges[range_name] = {
            'pos': config.pos,
            'base': config.base,
            'comment': config.comment,
            'priorities': {}  # Will be populated when loading priority files
        }

    return prio_ranges

