"""
Classificator parsing and management.
"""
import os
import csv
import logging
from typing import Dict, List, Set
from collections import OrderedDict

# Configure module logger
logger = logging.getLogger(__name__)

# Constants
EXPECTED_SHORT_COLUMNS = 3
EXPECTED_FULL_COLUMNS = 7


class ClassificatorManager:
    """Manages classificator types and their parsing."""

    def __init__(self) -> None:
        """Initialize classificator manager."""
        self.classificator: Dict[str, OrderedDict] = {}
        self.class_order: List[str] = []
        self.class_tree: Dict[str, str] = {}

    def parse_mapcss_mapping(self, data_dir: str) -> Set[str]:
        """
        Parse mapcss-mapping.csv file.

        Args:
            data_dir: Directory containing mapcss-mapping.csv

        Returns:
            Set of unique types for validation

        Raises:
            FileNotFoundError: If required files don't exist
            ValueError: If file format is invalid
        """
        # Validate input directory
        if not os.path.isdir(data_dir):
            raise FileNotFoundError(f"Data directory not found: {data_dir}")

        types_file_path = os.path.join(data_dir, 'types.txt')
        mapping_file_path = os.path.join(data_dir, 'mapcss-mapping.csv')

        if not os.path.isfile(mapping_file_path):
            raise FileNotFoundError(f"Mapping file not found: {mapping_file_path}")

        try:
            with open(types_file_path, "w") as types_file:
                cnt = 1
                unique_types_check = set()

                with open(mapping_file_path) as mapping_file:
                    for row_num, row in enumerate(csv.reader(mapping_file, delimiter=';'), start=1):
                        if len(row) <= 1 or row[0].startswith('#'):
                            # Allow for empty lines and comment lines starting with '#'.
                            continue

                        if len(row) == EXPECTED_SHORT_COLUMNS:
                            # Short format: type name, type id, x / replacement type name
                            tag = row[0].replace('|', '=')
                            obsolete = len(row[2].strip()) > 0
                            row = (row[0], '[{0}]'.format(tag), 'x' if obsolete else '', 'name', 'int_name', row[1], row[2] if row[2] != 'x' else '')

                        if len(row) != EXPECTED_FULL_COLUMNS:
                            raise ValueError(
                                f'Row {row_num}: Expecting {EXPECTED_SHORT_COLUMNS} or {EXPECTED_FULL_COLUMNS} columns, '
                                f'got {len(row)}: {";".join(row)}'
                            )

                        try:
                            type_id = int(row[5])
                        except ValueError:
                            raise ValueError(f'Row {row_num}: Invalid type id "{row[5]}"')

                        if type_id < cnt:
                            raise ValueError(
                                f'Row {row_num}: Wrong type id {type_id}, expected >= {cnt}: {";".join(row)}'
                            )

                        while type_id > cnt:
                            types_file.write("mapswithme\n")
                            cnt += 1
                        cnt += 1

                        cl = row[0].replace("|", "-")
                        if cl in unique_types_check and row[2] != 'x':
                            raise ValueError(f'Row {row_num}: Duplicate type: {row[0]}')

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
                            self.classificator[cl] = kv
                            self.class_order.append(cl)
                            unique_types_check.add(cl)
                            # Mark original type to distinguish it among replacing types.
                            types_file.write(f"*{row[0]}\n")
                        else:
                            # compatibility mode
                            if row[6]:
                                types_file.write(f"{row[6]}\n")
                            else:
                                types_file.write("mapswithme\n")

                        self.class_tree[cl] = row[0]

        except IOError as e:
            logger.error(f"Error reading/writing files: {e}")
            raise
        except csv.Error as e:
            logger.error(f"CSV parsing error: {e}")
            raise

        self.class_order.sort()
        return unique_types_check

    def get_mapcss_static_tags(self) -> Dict[str, bool]:
        """
        Get all mapcss static tags used in mapcss-mapping.csv.

        Returns:
            Dictionary with main_tag flags (True = appears first in types)
        """
        mapcss_static_tags = {}
        for v in list(self.classificator.values()):
            for i, t in enumerate(v.keys()):
                mapcss_static_tags[t] = mapcss_static_tags.get(t, True) and i == 0
        return mapcss_static_tags


def load_mapcss_dynamic_tags(data_dir: str) -> Set[str]:
    """
    Load mapcss dynamic tags from mapcss-dynamic.txt.

    Args:
        data_dir: Directory containing mapcss-dynamic.txt

    Returns:
        Set of dynamic tag names

    Raises:
        FileNotFoundError: If mapcss-dynamic.txt doesn't exist
    """
    dynamic_file_path = os.path.join(data_dir, 'mapcss-dynamic.txt')

    if not os.path.isfile(dynamic_file_path):
        raise FileNotFoundError(f"Dynamic tags file not found: {dynamic_file_path}")

    try:
        with open(dynamic_file_path) as dynamic_file:
            return {line.rstrip() for line in dynamic_file}
    except IOError as e:
        logger.error(f"Error reading dynamic tags file: {e}")
        raise
