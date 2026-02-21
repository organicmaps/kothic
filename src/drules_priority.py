"""
Priority management for drules.
"""
import os
import logging
from typing import Dict, Set, Optional, Tuple
from drules_config import (
    PRIO_OVERLAYS, PRIO_FG, PRIO_BG_TOP, PRIO_BG_BY_SIZE,
    OVERLAYS_MAX_PRIORITY, LAYER_PRIORITY_RANGE,
    COMMENT_AUTOFORMAT, COMMENT_RANGES_OVERVIEW
)

# Configure module logger
logger = logging.getLogger(__name__)


class PriorityManager:
    """Manages priority loading, calculation, and dumping for drules."""

    def __init__(self, prio_ranges: Dict[str, Dict[str, object]]) -> None:
        """
        Initialize priority manager.

        Args:
            prio_ranges: Dictionary of priority ranges with their configurations
        """
        self.prio_ranges = prio_ranges
        self.validation_errors_count = 0

    def get_priorities_filename(self, prio_range: str, path: str) -> str:
        """
        Get the filename for a priority range.

        Args:
            prio_range: Priority range identifier (e.g., 'overlays', 'FG')
            path: Directory path containing priority files

        Returns:
            Full path to the priority file
        """
        pos = self.prio_ranges[prio_range]["pos"]
        return os.path.join(path, f'priorities_{pos}_{prio_range}.prio.txt')

    def load_priorities(self, prio_range: str, path: str, classif: Set[str], compress: bool = False) -> None:
        """
        Load priorities from file for a given priority range.

        Args:
            prio_range: Priority range identifier
            path: Directory path containing priority files
            classif: Set of valid classificator types
            compress: Whether to compress priorities into continuous range

        Raises:
            FileNotFoundError: If priority file doesn't exist
            ValueError: If priority file is malformed
        """
        def print_warning(msg: str, line: str) -> None:
            """Log warning with context."""
            logger.warning(f'{msg} in {fname}:\n\t{line}')

        priority_max = OVERLAYS_MAX_PRIORITY if prio_range == PRIO_OVERLAYS else LAYER_PRIORITY_RANGE
        priority_min = -OVERLAYS_MAX_PRIORITY if prio_range == PRIO_OVERLAYS else 0
        fname = self.get_priorities_filename(prio_range, path)

        if not os.path.isfile(fname):
            raise FileNotFoundError(f"Priority file not found: {fname}")

        try:
            with open(fname, 'r') as f:
                group = []
                for line_num, line in enumerate(f, start=1):
                    line = line.strip()
                    # Strip comments
                    line = line.split('#', 1)[0].strip()
                    if not line:
                        continue

                    tokens = line.split()
                    if len(tokens) > 2:
                        print_warning(f'Line {line_num}: skipping malformed line', line)
                        continue

                    if tokens[0] == "===":
                        if len(tokens) < 2:
                            print_warning(f'Line {line_num}: missing priority value', line)
                            continue

                        try:
                            priority = int(tokens[1])
                        except ValueError:
                            print_warning(f'Line {line_num}: skipping invalid priority value', line)
                            continue

                        if priority_min <= priority < priority_max:
                            if len(group):
                                for key in group:
                                    self.prio_ranges[prio_range]['priorities'][key] = priority
                            else:
                                print_warning(f'Line {line_num}: skipping empty priority group', line)
                        else:
                            print_warning(
                                f'Line {line_num}: skipping out of [{priority_min};{priority_max}) range priority value',
                                line
                            )
                        group = []
                    else:
                        cl = tokens[0]
                        object_id = ''
                        oid_pos = cl.find('::')
                        if oid_pos != -1:
                            object_id = cl[oid_pos:]
                            cl = cl[0:oid_pos]

                        if cl not in classif:
                            print_warning(f'Line {line_num}: unknown classificator type', line)

                        key = (cl, object_id)
                        if key in self.prio_ranges[prio_range]['priorities']:
                            old_prio = self.prio_ranges[prio_range]["priorities"][key]
                            print_warning(
                                f'Line {line_num}: overriding previously set priority value {old_prio}',
                                line
                            )
                        group.append(key)

                if len(group):
                    print_warning(f'skipping last types group with no priority set', str(group))

        except IOError as e:
            logger.error(f"Error reading priority file {fname}: {e}")
            raise

        if prio_range == PRIO_OVERLAYS:
            self._adjust_overlay_priorities()

        if compress:
            self._compress_priorities(prio_range, priority_max)

    def _adjust_overlay_priorities(self) -> None:
        """
        Adjust overlay priorities for captions and pathtexts.

        Ensures that captions don't have higher priority than their icons,
        and pathtexts don't exceed their shields.
        """
        for key in self.prio_ranges[PRIO_OVERLAYS]['priorities'].keys():
            main_prio_id = None

            # Caption should not exceed icon priority
            if key[1].startswith('caption'):
                main_prio_id = (key[0], key[1].replace('caption', 'icon'))
            # Pathtext should not exceed shield priority
            if key[1].startswith('pathtext'):
                main_prio_id = (key[0], key[1].replace('pathtext', 'shield'))

            if main_prio_id is not None and main_prio_id in self.prio_ranges[PRIO_OVERLAYS]['priorities']:
                main_prio = self.prio_ranges[PRIO_OVERLAYS]['priorities'][main_prio_id]
                if self.prio_ranges[PRIO_OVERLAYS]['priorities'][key] > main_prio:
                    logger.warning(f'{key} priority is higher than {main_prio_id}, making it equal')
                    self.prio_ranges[PRIO_OVERLAYS]['priorities'][key] = main_prio

    def _compress_priorities(self, prio_range: str, priority_max: int) -> None:
        """
        Compress priorities into a continuous range to avoid gaps.

        Args:
            prio_range: Priority range identifier
            priority_max: Maximum priority value
        """
        logger.info(f'Compressing {prio_range} priorities into a (0;{priority_max}) range:')
        unique_prios = set(self.prio_ranges[prio_range]['priorities'].values())
        logger.info(f'\tunique priorities values: {len(unique_prios)}')

        # Keep gaps at the range borders
        base_idx = 1
        if 0 not in unique_prios:
            base_idx = 0
            unique_prios.add(0)
        unique_prios.add(priority_max)

        step = min(priority_max / len(unique_prios), 10)
        logger.info(f'\tnew step between priorities: {step}')
        unique_prios_sorted = sorted(unique_prios)

        for prio_id in self.prio_ranges[prio_range]['priorities'].keys():
            idx = unique_prios_sorted.index(self.prio_ranges[prio_range]['priorities'][prio_id])
            self.prio_ranges[prio_range]['priorities'][prio_id] = int(step * (base_idx + idx))

    def get_drape_priority(self, cl: str, dr_type: str, object_id: str,
                          auto_dr_type: Optional[str] = None,
                          auto_comment: Optional[str] = None,
                          auto_prio_mod: int = 0) -> int:
        """
        Get drape priority for a given type.

        Args:
            cl: Classification type
            dr_type: Draw rule type
            object_id: Object identifier
            auto_dr_type: Automatic draw rule type
            auto_comment: Automatic comment
            auto_prio_mod: Automatic priority modifier

        Returns:
            Priority value
        """
        if object_id == '::default':
            object_id = ''
        prio_id = (cl, object_id)

        ranges_to_check = (PRIO_OVERLAYS, )
        if dr_type == 'line':
            ranges_to_check = (PRIO_FG, PRIO_BG_TOP)
        elif dr_type == 'area':
            ranges_to_check = (PRIO_BG_BY_SIZE, PRIO_BG_TOP, PRIO_FG)

        for r in ranges_to_check:
            if prio_id in self.prio_ranges[r]['priorities']:
                priority = self.prio_ranges[r]['priorities'][prio_id]
                if auto_dr_type is not None:
                    min_priority = -OVERLAYS_MAX_PRIORITY if r == PRIO_OVERLAYS else 0
                    priority = max(priority + auto_prio_mod, min_priority)
                    auto_prio_id = (cl, object_id, auto_dr_type, auto_comment)
                    self.prio_ranges[r]['priorities'][auto_prio_id] = priority
                return priority + self.prio_ranges[r]['base']

        logger.error(f'Priority is not set for {dr_type} {cl}{object_id}')
        self.validation_errors_count += 1
        return 0

    def dump_priorities(self, prio_range: str, path: str, maxzoom: int,
                       visibilities: Dict[str, Dict[Tuple[str, Optional[str]], Dict[str, Set[int]]]]) -> None:
        """
        Dump priorities to file with visibility information.

        The output format matches the original libkomwm.py exactly:
        - auto_comment formatted as "dr_type(auto_comment)" e.g. "caption(optional)"
        - other_drules sorted by dr_types_order then by object_id
        - sorted by (OVERLAYS_MAX_PRIORITY - priority, cl, object_id) for consistent output
        - one blank line after each priority group

        Args:
            prio_range: Priority range identifier
            path: Directory path where to write the file
            maxzoom: Maximum zoom level
            visibilities: Dictionary mapping classifications to visibility info

        Raises:
            IOError: If file cannot be written
        """
        from drules_utils import prettify_zooms

        # Order in which drule types appear in comments, matching old libkomwm
        dr_types_order = (
            ('icon', 'caption', 'pathtext', 'shield', 'line', 'area')
            if prio_range == PRIO_OVERLAYS
            else ('line', 'area', 'icon', 'caption', 'pathtext', 'shield')
        )

        # Sort key matching old libkomwm exactly:
        # primary: descending priority (OVERLAYS_MAX_PRIORITY - v so higher prio sorts first)
        # secondary: classification name
        # tertiary: object_id
        def priority_sort_key(item: Tuple) -> Tuple:
            k, v = item
            return (OVERLAYS_MAX_PRIORITY - v, k[0], k[1])

        fname = self.get_priorities_filename(prio_range, path)

        try:
            with open(fname, 'w') as outfile:
                # Write header: all 3 comment sections joined and prefixed with "# "
                # exactly as old code: COMMENT_AUTOFORMAT + range comment + COMMENT_RANGES_OVERVIEW
                range_comment = self.prio_ranges[prio_range]['comment']
                full_comment = COMMENT_AUTOFORMAT + range_comment + COMMENT_RANGES_OVERVIEW
                for s in full_comment.splitlines():
                    outfile.write(f'# {s}'.rstrip() + '\n')
                outfile.write('\n')

                if not self.prio_ranges[prio_range]['priorities']:
                    return

                # Comment block inserted before the first negative-priority group (overlays only)
                comment_auto_captions = (
                    '\nAll automatic optional captions priorities are below 0.\n'
                    'They follow the order of their correspoding icons.\n\n'
                )

                priorities_sorted = sorted(
                    self.prio_ranges[prio_range]['priorities'].items(),
                    key=priority_sort_key
                )

                group_prio = priorities_sorted[0][1]
                group = ''
                group_comment = '# '

                for k, v in priorities_sorted:
                    if v != group_prio:
                        # Insert auto-captions comment block before first negative group
                        if prio_range == PRIO_OVERLAYS and comment_auto_captions and group_prio < 0:
                            for s in comment_auto_captions.splitlines():
                                outfile.write(f'# {s.strip()}'.rstrip() + '\n')
                            outfile.write('\n')
                            comment_auto_captions = None

                        outfile.write(f'{group}{group_comment}=== {group_prio}\n\n')
                        group_prio = v
                        group = ''
                        group_comment = '# '

                    cl = k[0]
                    object_id = k[1]
                    auto_dr_type = k[2] if len(k) == 4 else None
                    auto_comment = k[3] if len(k) == 4 else None

                    line_drules = ''
                    other_drules = ''

                    if cl in visibilities:
                        # Sort by dr_types_order then by object_id — matches old libkomwm
                        sorted_dr_type_comments = sorted(
                            visibilities[cl].keys(),
                            key=lambda drt: (dr_types_order.index(drt[0]), )
                        )
                        for dr_type_comment in sorted_dr_type_comments:
                            for oid in sorted(visibilities[cl][dr_type_comment].keys()):
                                dr_type, dr_auto_comment = dr_type_comment
                                dr_zoom = prettify_zooms(
                                    visibilities[cl][dr_type_comment][oid], maxzoom
                                )

                                # Format: "dr_type" or "dr_type::oid"
                                dr_info = dr_type + oid
                                # Append auto_comment in parentheses: "caption(optional)"
                                if dr_auto_comment is not None:
                                    dr_info = f'{dr_info}({dr_auto_comment})'
                                dr_info += ' ' + dr_zoom

                                is_auto_dr_match = dr_type == auto_dr_type and dr_auto_comment == auto_comment
                                is_not_auto_dr = auto_dr_type is None and dr_auto_comment is None
                                is_suitable_for_range = (
                                    (prio_range == PRIO_OVERLAYS and dr_type in ('icon', 'caption', 'pathtext', 'shield')) or
                                    (prio_range in (PRIO_FG, PRIO_BG_TOP) and dr_type in ('line', 'area')) or
                                    (prio_range == PRIO_BG_BY_SIZE and dr_type == 'area')
                                )

                                if oid == object_id and (is_auto_dr_match or (is_not_auto_dr and is_suitable_for_range)):
                                    if line_drules:
                                        line_drules += ' and '
                                    line_drules += dr_info
                                else:
                                    if other_drules:
                                        other_drules += ', '
                                    other_drules += dr_info

                    if object_id:
                        cl += object_id

                    if not line_drules:
                        if other_drules:
                            line_drules = "WARNING: no drule defined for the priority"
                        else:
                            line_drules = "WARNING: no style defined (the type will be not included into map data)"
                        logger.warning(f'{line_drules} for {cl} in {prio_range}')

                    info = '# ' + line_drules
                    if other_drules:
                        info += f' (also has {other_drules})'

                    if auto_dr_type is None:
                        group_comment = ''
                    else:
                        cl = '# ' + cl

                    group += f'{cl:50}  {info}\n'

                # Write final group — single trailing newline, matching old libkomwm
                outfile.write(f'{group}{group_comment}=== {group_prio}\n')

            logger.debug(f"Dumped priorities to {fname}")

        except IOError as e:
            logger.error(f"Error writing priority file {fname}: {e}")
            raise


