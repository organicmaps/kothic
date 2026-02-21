"""
File I/O operations for drules generation.
"""
import os
import logging
from typing import Set, List, Tuple, Callable, Dict

# Configure module logger
logger = logging.getLogger(__name__)


class FileIOManager:
    """Manages reading and writing of various drules-related files."""

    def __init__(self, data_dir: str) -> None:
        """
        Initialize file I/O manager.

        Args:
            data_dir: Base data directory

        Raises:
            ValueError: If data_dir is invalid
        """
        if not data_dir:
            raise ValueError("data_dir cannot be empty")
        if not os.path.isdir(data_dir):
            raise FileNotFoundError(f"Data directory not found: {data_dir}")

        # Normalize and validate path
        self.data_dir = os.path.normpath(os.path.abspath(data_dir))
        logger.debug(f"FileIOManager initialized with data_dir: {self.data_dir}")

    def load_colors(self) -> Set[int]:
        """
        Load colors from colors.txt.

        Returns:
            Set of color integers
        """
        colors_file_name = os.path.join(self.data_dir, 'colors.txt')
        colors = set()

        if not os.path.exists(colors_file_name):
            logger.debug(f"Colors file not found, starting empty: {colors_file_name}")
            return colors

        try:
            with open(colors_file_name, "r") as colors_in_file:
                for line_num, color_line in enumerate(colors_in_file, start=1):
                    try:
                        colors.add(int(color_line.strip()))
                    except ValueError:
                        logger.warning(f"Invalid color on line {line_num}: {color_line.strip()}")
        except IOError as e:
            logger.error(f"Error reading colors file {colors_file_name}: {e}")
            raise

        logger.debug(f"Loaded {len(colors)} colors")
        return colors

    def save_colors(self, colors: Set[int]) -> None:
        """
        Save colors to colors.txt.

        Args:
            colors: Set of color integers
        """
        colors_file_name = os.path.join(self.data_dir, 'colors.txt')

        try:
            with open(colors_file_name, "w") as colors_file:
                for c in sorted(colors):
                    colors_file.write(f"{c}\n")
            logger.debug(f"Saved {len(colors)} colors to {colors_file_name}")
        except IOError as e:
            logger.error(f"Error writing colors file {colors_file_name}: {e}")
            raise

    def load_patterns(self) -> Tuple[List[List[float]], Callable[[List[float]], None]]:
        """
        Load patterns from patterns.txt.

        Returns:
            Tuple of (patterns list, add_pattern function)
        """
        patterns: List[List[float]] = []
        patterns_set: Set[Tuple[float, ...]] = set()  # For O(1) duplicate checking

        def add_pattern(dashes: List[float]) -> None:
            """Add pattern if not already present."""
            if dashes:
                dashes_tuple = tuple(dashes)
                if dashes_tuple not in patterns_set:
                    patterns_set.add(dashes_tuple)
                    patterns.append(dashes)

        patterns_file_name = os.path.join(self.data_dir, 'patterns.txt')

        if not os.path.exists(patterns_file_name):
            logger.debug(f"Patterns file not found, starting empty: {patterns_file_name}")
            return patterns, add_pattern

        try:
            with open(patterns_file_name, "r") as patterns_in_file:
                for line_num, patterns_line in enumerate(patterns_in_file, start=1):
                    try:
                        pattern = [float(x) for x in patterns_line.split()]
                        if pattern:  # Skip empty lines
                            add_pattern(pattern)
                    except ValueError as e:
                        logger.warning(f"Invalid pattern on line {line_num}: {e}")
        except IOError as e:
            logger.error(f"Error reading patterns file {patterns_file_name}: {e}")
            raise

        logger.debug(f"Loaded {len(patterns)} patterns")
        return patterns, add_pattern

    def save_patterns(self, patterns: List[List[float]]) -> None:
        """
        Save patterns to patterns.txt.

        Args:
            patterns: List of pattern arrays
        """
        patterns_file_name = os.path.join(self.data_dir, 'patterns.txt')

        try:
            with open(patterns_file_name, "w") as patterns_file:
                for p in patterns:
                    patterns_file.write(f"{' '.join(str(elem) for elem in p)}\n")
            logger.debug(f"Saved {len(patterns)} patterns to {patterns_file_name}")
        except IOError as e:
            logger.error(f"Error writing patterns file {patterns_file_name}: {e}")
            raise

    def save_visibility(self, visibility: Dict[str, str], maxzoom: int) -> None:
        """
        Save visibility.txt file.

        This creates a hierarchical tree structure showing visibility ranges for each classification.

        Args:
            visibility: Visibility dictionary mapping classification paths to zoom strings
            maxzoom: Maximum zoom level

        """
        import functools

        # Build tree nodes (intermediate levels)
        visnodes = set()
        for k in visibility.keys():
            vis = k.split("|")
            # Add all parent nodes in the path
            for i in range(1, len(vis) - 1):
                visnodes.add("|".join(vis[0:i]) + "|")
        viskeys = list(set(list(visibility.keys()) + list(visnodes)))

        # Sort keys for consistent output
        def compare_keys(a: str, b: str) -> int:
            """Compare two visibility keys for sorting."""
            if a == b:
                return 0
            a = a.replace("|", "-")
            b = b.replace("|", "-")
            return 1 if a > b else -1

        viskeys.sort(key=functools.cmp_to_key(compare_keys))

        visibility_file_path = os.path.join(self.data_dir, 'visibility.txt')
        classificator_file_path = os.path.join(self.data_dir, 'classificator.txt')

        try:
            with open(visibility_file_path, "w") as visibility_file, \
                 open(classificator_file_path, "w") as classificator_file:

                oldoffset = ""
                for k in viskeys:
                    # Calculate indentation based on tree depth
                    offset = "    " * (k.count("|") - 1)

                    # Write closing braces for completed branches
                    for i in range(len(oldoffset) // 4, len(offset) // 4, -1):
                        indent = "    " * i
                        visibility_file.write(f"{indent}{{}}\n")
                        classificator_file.write(f"{indent}{{}}\n")
                    oldoffset = offset

                    # Determine if this is a branch node or leaf
                    end = "+" if k in visnodes else "-"

                    # Extract the classification name (last element before final |)
                    name = k.split("|")[-2]

                    # Write to both files
                    vis_string = visibility.get(k, "0" * (maxzoom + 1))
                    visibility_file.write(f"{offset}{name}  {vis_string}  {end}\n")
                    classificator_file.write(f"{offset}{name}  {end}\n")

                # Close remaining braces
                for i in range(len(offset) // 4, 0, -1):
                    indent = "    " * i
                    visibility_file.write(f"{indent}{{}}\n")
                    classificator_file.write(f"{indent}{{}}\n")

            logger.debug(f"Saved visibility files: {visibility_file_path}, {classificator_file_path}")
        except IOError as e:
            logger.error(f"Error writing visibility files: {e}")
            raise

