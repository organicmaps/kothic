"""
Visibility tracking for drules generation.

This module tracks which drawing rules (icons, captions, lines, areas) are visible
at which zoom levels for each classification type. This information is used to
generate visibility files that optimize map rendering performance.
"""
import logging
from typing import Dict, Set, Tuple, Optional

# Configure module logger
logger = logging.getLogger(__name__)

# Constants
DEFAULT_OBJECT_ID = '::default'


# Type aliases for complex nested structures
VisibilityKey = Tuple[str, Optional[str]]  # (draw_type, auto_comment)
ObjectVisibility = Dict[str, Set[int]]  # object_id -> zoom_levels
ClassificationVisibility = Dict[VisibilityKey, ObjectVisibility]
VisibilityData = Dict[str, ClassificationVisibility]  # classification -> visibility


class VisibilityTracker:
    """
    Tracks visibility of drawing rules across zoom levels.

    This class maintains a nested dictionary structure that maps:
    classification -> (draw_type, auto_comment) -> object_id -> set of zoom levels

    Example:
        tracker = VisibilityTracker()
        tracker.store_visibility('highway-primary', 'line', '', 10)
        tracker.store_visibility('amenity-restaurant', 'icon', '', 14)
    """

    def __init__(self) -> None:
        """
        Initialize visibility tracker.

        The visibilities dictionary structure:
        {
            'highway-primary': {
                ('line', None): {'': {10, 11, 12, 13, 14, ...}},
                ('line', 'bridge'): {'::dash': {14, 15, 16}}
            },
            'amenity-restaurant': {
                ('icon', None): {'': {14, 15, 16, ...}},
                ('caption', 'optional'): {'': {16, 17, 18}}
            }
        }
        """
        self.visibilities: VisibilityData = {}
        self._stats = {
            'total_entries': 0,
            'classifications': 0,
            'zoom_levels_tracked': 0
        }

    def store_visibility(self, cl: str, dr_type: str, object_id: str, zoom: int,
                         auto_comment: Optional[str] = None) -> None:
        """
        Store visibility information for a drawing rule.

        Args:
            cl: Classification type (e.g., 'highway-primary', 'amenity-restaurant')
            dr_type: Draw rule type ('line', 'area', 'icon', 'caption', 'pathtext', 'shield')
            object_id: Object identifier (e.g., '::dash', '::bridge', or '' for default)
            zoom: Zoom level where this rule is visible (typically 0-20)
            auto_comment: Optional automatic comment (e.g., 'optional', 'casing')

        Raises:
            ValueError: If zoom is negative

        Note:
            The special object_id '::default' is normalized to empty string.
        """
        # Validate inputs
        if not cl:
            raise ValueError("Classification type cannot be empty")
        if not dr_type:
            raise ValueError("Draw rule type cannot be empty")
        if zoom < 0:
            raise ValueError(f"Zoom level cannot be negative: {zoom}")

        # Normalize default object_id
        if object_id == DEFAULT_OBJECT_ID:
            object_id = ''

        # Create nested structure using setdefault for cleaner code
        dr_type_comment = (dr_type, auto_comment)

        # Initialize nested dictionaries if needed
        if cl not in self.visibilities:
            self.visibilities[cl] = {}
            self._stats['classifications'] += 1

        if dr_type_comment not in self.visibilities[cl]:
            self.visibilities[cl][dr_type_comment] = {}

        if object_id not in self.visibilities[cl][dr_type_comment]:
            self.visibilities[cl][dr_type_comment][object_id] = set()
            self._stats['total_entries'] += 1

        # Add zoom level to the set
        self.visibilities[cl][dr_type_comment][object_id].add(zoom)
        self._stats['zoom_levels_tracked'] += 1

        logger.debug(
            f"Stored visibility: {cl}/{dr_type}:{auto_comment or 'none'}/{object_id or 'default'} @ z{zoom}"
        )

    def validate_visibilities(self, maxzoom: int) -> int:
        """
        Validate visibility data for consistency and detect potential issues.

        Args:
            maxzoom: Maximum zoom level in the system (used for validation)

        Returns:
            Number of validation warnings found (not necessarily errors)

        Note:
            This performs soft validation - warnings are logged but don't prevent
            operation. Common checks:
            - Captions without corresponding icons (allowed but unusual)
            - Zoom levels exceeding maxzoom
            - Empty visibility sets
        """
        warnings_count = 0

        for cl, type_dict in self.visibilities.items():
            # Collect zoom levels for icons and captions
            icon_zooms = set()
            caption_zooms = set()

            for (dr_type, auto_comment), objects in type_dict.items():
                # Check for zoom levels exceeding maxzoom
                for object_id, zooms in objects.items():
                    if not zooms:
                        logger.warning(f"Empty zoom set for {cl}/{dr_type}/{object_id}")
                        warnings_count += 1

                    for zoom in zooms:
                        if zoom > maxzoom:
                            logger.warning(
                                f"Zoom level {zoom} exceeds maxzoom {maxzoom} for {cl}/{dr_type}/{object_id}"
                            )
                            warnings_count += 1

                # Track icon and caption zooms for relationship validation
                if dr_type == 'icon':
                    icon_zooms.update({z for zooms in objects.values() for z in zooms})
                elif dr_type == 'caption':
                    caption_zooms.update({z for zooms in objects.values() for z in zooms})

            # Check for captions without icons (unusual but allowed)
            caption_only_zooms = caption_zooms - icon_zooms
            if caption_only_zooms:
                logger.debug(
                    f"Classification {cl} has captions without icons at zoom levels: "
                    f"{sorted(caption_only_zooms)}"
                )
                # This is informational, not counted as a warning

        if warnings_count > 0:
            logger.info(f"Visibility validation found {warnings_count} warning(s)")
        else:
            logger.debug("Visibility validation passed with no warnings")

        return warnings_count

    def get_visibilities(self) -> VisibilityData:
        """
        Get the complete visibilities dictionary.

        Returns:
            The nested dictionary structure containing all visibility data.
            Structure: classification -> (draw_type, auto_comment) -> object_id -> zoom_levels
        """
        return self.visibilities

    def get_statistics(self) -> Dict[str, int]:
        """
        Get statistics about tracked visibility data.

        Returns:
            Dictionary with statistics:
            - total_entries: Number of unique (classification, type, object_id) combinations
            - classifications: Number of unique classification types
            - zoom_levels_tracked: Total number of zoom level recordings
        """
        return self._stats.copy()

    def clear(self) -> None:
        """Clear all stored visibility data and reset statistics."""
        self.visibilities.clear()
        self._stats = {
            'total_entries': 0,
            'classifications': 0,
            'zoom_levels_tracked': 0
        }
        logger.debug("Visibility tracker cleared")
