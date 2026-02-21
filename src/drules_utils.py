"""
Utility functions for drules generation.
"""
import logging
from typing import Set, Dict, Optional
import mapcss.webcolors

# Configure module logger
logger = logging.getLogger(__name__)


def to_boolean(s: str) -> Optional[bool]:
    """
    Convert string to boolean.

    Args:
        s: String value to convert

    Returns:
        - True if s is "true" or "yes"
        - False if s is "false" or "no"
        - None if s is invalid or not a string
    """
    if not isinstance(s, str):
        return None

    s_lower = s.lower()
    if s_lower in ("true", "yes"):
        return True
    elif s_lower in ("false", "no"):
        return False
    else:
        return None


def mwm_encode_color(colors: Set[int], st: Dict[str, str], prefix: str = '', default: str = 'black') -> int:
    """
    Encode color from style properties to ARGB integer format.

    Args:
        colors: Set to add the color to (modified in-place)
        st: Style dictionary containing color and opacity values
        prefix: Prefix for property lookup (e.g., 'fill', 'stroke')
        default: Default color value if not specified

    Returns:
        Encoded color as ARGB integer (opacity + RGB hex)

    Note:
        TODO: Refactoring idea - MapCSS converts colors from hex to float during parsing,
        then we convert back to hex here. Could avoid Hex->Float->Hex by keeping hex values.
    """
    property_prefix = f"{prefix}-" if prefix else ""

    try:
        # Get opacity (0.0-1.0) and convert to alpha byte (0-255)
        opacity_str = st.get(f"{property_prefix}opacity", "1")
        opacity_float = float(opacity_str)
        opacity_float = max(0.0, min(1.0, opacity_float))  # Clamp to valid range
        alpha = 255 - int(255 * opacity_float)
        opacity_hex = f"{alpha:02x}"

        # Get color hex value
        color_value = st.get(f"{property_prefix}color", default)
        color_hex = mapcss.webcolors.webcolors.whatever_to_hex(color_value)[1:]  # Strip '#'

        # Combine to ARGB format
        result = int(opacity_hex + color_hex, 16)
        colors.add(result)
        return result

    except (ValueError, TypeError, AttributeError) as e:
        logger.warning(f"Error encoding color with prefix '{prefix}': {e}. Using default.")
        # Fallback to opaque default color
        try:
            default_hex = mapcss.webcolors.webcolors.whatever_to_hex(default)[1:]
            result = int("00" + default_hex, 16)
            colors.add(result)
            return result
        except Exception:
            # Last resort: opaque black
            return 0x00000000


def mwm_encode_image(st: Dict[str, str], prefix: str = 'icon') -> Optional[str]:
    """
    Encode image handle from style properties.

    Args:
        st: Style dictionary
        prefix: Prefix for property lookup (default: 'icon')
        bgprefix: Background prefix (default: 'symbol', currently unused)

    Returns:
        Handle if image exists, None otherwise

    Note:
        Returns tuple for backward compatibility with existing code.
    """
    property_key = f"{prefix}-image" if prefix else "image"

    if property_key not in st:
        return None

    image_path = st.get(property_key, "")
    if not image_path:
        return None

    # Strip .svg extension if present
    if image_path.endswith(".svg"):
        handle = image_path[:-4]
    else:
        handle = image_path

    return handle


def prettify_zooms(zooms: Set[int], maxzoom: int) -> str:
    """
    Convert set of zoom levels to human-readable string format.

    Args:
        zooms: Set of zoom levels (e.g., {1, 2, 3, 7, 9, 10, 11})
        maxzoom: Maximum zoom level in the system

    Returns:
        Formatted zoom string with ranges (e.g., "z1-3,z7,z9-11")
        - Single zoom: "z7"
        - Range to max: "z15-"
        - Range: "z10-14"
        - Multiple: "z1-3,z7,z10-"
    """
    def add_zrange(first: int, last: int, result: str, maxzoom: int) -> str:
        """Add a zoom range to the result string."""
        if last == maxzoom:
            # Range to maximum: "z15-"
            zrange = f'z{first}-'
        elif first == last:
            # Single zoom: "z7"
            zrange = f'z{first}'
        else:
            # Range: "z10-14"
            zrange = f'z{first}-{last}'

        # Add separator if not first range
        if result:
            result += ','
        result += zrange
        return result

    if not zooms:
        return ''

    sorted_zooms = sorted(zooms)
    result = ''

    # Build ranges by tracking consecutive zooms
    first = last = sorted_zooms[0]
    for z in sorted_zooms[1:]:
        if z == last + 1:
            # Continue current range
            last = z
        else:
            # End current range and start new one
            result = add_zrange(first, last, result, maxzoom)
            first = last = z

    # Add final range
    result = add_zrange(first, last, result, maxzoom)
    return result

