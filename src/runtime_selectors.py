"""Partition feature predicates that the drawing-rule reader can test both ways."""

MAX_POPULATION = (1 << 64) - 1


def implied(predicate, decided, present):
    if predicate == decided:
        return present
    kind, value = predicate
    other_kind, other_value = decided
    if kind == other_kind == 'extra_tag':
        if present and other_value.startswith(value + '='):
            return True
        if not present and value.startswith(other_value + '='):
            return False
    if kind == other_kind == 'population':
        if present and other_value >= value:
            return True
        if not present and other_value <= value:
            return False
    return None


def simplify(clause):
    """Drop inferred predicates and discard an impossible conjunction."""
    unchecked = list(dict.fromkeys(clause))
    result = []
    while unchecked:
        predicate, present = unchecked.pop(0)
        if predicate[0] == 'population' and predicate[1] in (0, MAX_POPULATION + 1):
            if present != (predicate[1] == 0):
                return None
            continue
        for other, other_present in (*unchecked, *result):
            known = implied(predicate, other, other_present)
            if known is not None:
                if known != present:
                    return None
                break
        else:
            result.append((predicate, present))
    return tuple(result)


def ordinary_clauses(condition):
    tag = condition.params[0]
    op = condition.type
    if tag == 'name' and op in ('set', 'unset'):
        predicate = ('name', None)
        return (((predicate, op == 'set'),),)
    if tag == 'population':
        if op in ('set', 'unset'):
            predicate = ('population', 1)
            return (((predicate, op == 'set'),),)
        if op in ('eq', 'ne', '<', '<=', '>', '>='):
            value = condition.params[1].strip()
            if value.isascii() and value.isdecimal() and len(value.lstrip('0')) <= 20:
                number = int(value.lstrip('0') or '0')
                if number <= MAX_POPULATION:
                    at_least = ('population', number)
                    above = ('population', number + 1)
                    if op == 'eq':
                        return (((at_least, True), (above, False)),)
                    if op == 'ne':
                        return (((at_least, False),), ((above, True),))
                    predicate = at_least if op in ('<', '>=') else above
                    return (((predicate, op in ('>', '>=')),),)
    # For example, bbox_area cannot express a complement on non-area features.
    raise ValueError(f'Unsupported runtime condition in a selector cascade: {condition}')


def selector_clauses(rule):
    classifier = simplify((('extra_tag', path), present) for path, present in rule.classifier_conditions)
    if classifier is None:
        return ()
    clauses = (classifier,)
    for condition in rule.runtime_conditions or ():
        additions = ordinary_clauses(condition)
        clauses = tuple(dict.fromkeys(new for clause in clauses for addition in additions
                                      if (new := simplify(clause + addition)) is not None))
    return clauses


def decision_groups(choices, requirements):
    for choosers in choices:
        for _, matches in choosers:
            if len({layer for _, layer in matches}) == 1:
                # A single layer needs only one satisfied alternative.
                yield tuple(clause for rule, _ in matches for clause in requirements[rule])
            else:
                # The earliest eligible selector for each layer determines the cascade order.
                yield from (requirements[rule] for rule, _ in matches)


def partition(groups):
    if not groups:
        yield ()
        return

    first_clause = groups[0][0]
    predicate, _ = first_clause[0]
    for present in (False, True):
        remaining = []
        for clauses in groups:
            alternatives = []
            for clause in clauses:
                pending = []
                for test, required in clause:
                    known = implied(test, predicate, present)
                    if known is None:
                        pending.append((test, required))
                    elif known != required:
                        break
                else:
                    alternatives.append(tuple(pending))
            if alternatives and all(alternatives):
                remaining.append(tuple(alternatives))
        for assignments in partition(remaining):
            yield ((predicate, present), *assignments)


def emit_guard(predicate, present):
    kind, value = predicate
    if kind == 'extra_tag':
        return f'extra_tag{"=" if present else "!="}{value}'
    if kind == 'population':
        return f'population{">=" if present else "<"}{value}'
    assert kind == 'name'
    return 'name' if present else '!name'


def runtime_variants(choices):
    requirements = {
        rule: selector_clauses(rule)
        for choosers in choices for _, matches in choosers for rule, _ in matches
    }
    groups = tuple(dict.fromkeys(group for group in decision_groups(choices, requirements) if group and all(group)))
    predicates = {predicate for clauses in requirements.values() for clause in clauses for predicate, _ in clause}
    for assignments in partition(groups):
        resolved = {}
        for decided, present in assignments:
            for predicate in predicates:
                if (known := implied(predicate, decided, present)) is not None:
                    resolved[predicate] = known
        enabled = {rule for rule, clauses in requirements.items()
                   if any(all(resolved.get(predicate) == required for predicate, required in clause)
                          for clause in clauses)}
        yield [emit_guard(predicate, present) for predicate, present in assignments], enabled.__contains__
