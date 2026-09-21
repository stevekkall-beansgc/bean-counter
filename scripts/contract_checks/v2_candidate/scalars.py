"""Schema-directed scalar checks, including canonical JSON embedded as bytes.

These keywords are normative candidate validation, not JSON Schema annotations
that callers may ignore. Extension values are not interpreted as field names.
"""
import re
import unicodedata
from datetime import datetime
from fractions import Fraction
from math import gcd
import jsonschema
from profile import canonical, strict


def decimal(value, positive=False):
    if not isinstance(value, str) or len(value.encode()) > 64 or not re.fullmatch(r'(0|[1-9][0-9]*)(\.[0-9]*[1-9])?', value):
        raise ValueError('DECIMAL_CANONICAL')
    whole, _, fraction = value.partition('.')
    if len(fraction) > 18 or len((whole + fraction).lstrip('0')) > 30:
        raise ValueError('DECIMAL_PRECISION')
    if positive and value == '0':
        raise ValueError('QUANTITY')


def check_scalar(kind, value):
    if kind in ('text', 'source'):
        if not isinstance(value, str) or not value or any(unicodedata.category(c) == 'Cc' for c in value):
            raise ValueError('IDENTIFIER')
        if kind == 'source' and (not re.match(r'^[A-Za-z][A-Za-z0-9+.-]*:', value) or any(c.isspace() for c in value)):
            raise ValueError('SOURCE')
    elif kind in ('decimal', 'positive-decimal', 'decimal-percent'):
        decimal(value, kind == 'positive-decimal')
        if kind == 'decimal-percent' and Fraction(value)>100:
            raise ValueError('POLICY_PERCENT_RANGE')
    elif kind == 'uint':
        if not isinstance(value, str) or not re.fullmatch(r'0|[1-9][0-9]*', value) or int(value) > 9223372036854775807:
            raise ValueError('COUNTER')
    elif kind == 'time':
        if not isinstance(value, str) or not re.fullmatch(r'\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{6}Z', value):
            raise ValueError('TIME')
        datetime.strptime(value, '%Y-%m-%dT%H:%M:%S.%fZ')
    elif kind == 'ratio':
        if not isinstance(value, dict) or set(value) != {'numerator', 'denominator'}:
            raise ValueError('RATIO')
        integer_spelling(value['numerator'],True,156)
        integer_spelling(value['denominator'],False,155)
        n, d = int(value['numerator']), int(value['denominator'])
        if d <= 0 or gcd(n, d) != 1 or max(abs(n).bit_length(), d.bit_length()) > 512:
            raise ValueError('RATIO')
    elif kind == 'nonnegative-money':
        integer_spelling(value['atoms'],True,31)
        if int(value['atoms']) < 0:
            raise ValueError('NEGATIVE_LIMIT')
    elif kind in ('atoms','nonnegative-atoms'):
        integer_spelling(value,kind=='atoms',31 if kind=='atoms' else 30)
        if abs(int(value))>10**30-1:raise ValueError('ATOMS')
    elif kind == 'slug':
        if not isinstance(value,str) or not re.fullmatch(r'[a-z][a-z0-9_.-]{0,63}',value):raise ValueError('SLUG')
    else:
        raise ValueError('UNKNOWN_SCALAR_CONSTRAINT:' + kind)


def integer_spelling(value, signed, limit):
    pattern=r'0|-?[1-9][0-9]*' if signed else r'0|[1-9][0-9]*'
    if not isinstance(value,str) or len(value)>limit or not re.fullmatch(pattern,value):
        raise ValueError('CANONICAL_INTEGER')


def whole_pattern(validator, pattern, value, schema):
    # Draft JSON Schema pattern uses search; `$` alone also permits a final LF.
    # Every candidate pattern is a whole canonical spelling, never a substring.
    if isinstance(value,str) and not re.fullmatch(pattern,value):
        yield jsonschema.ValidationError('CANONICAL_SPELLING')


def scalar(validator, kind, value, schema):
    try:
        check_scalar(kind, value)
    except (ValueError, TypeError, KeyError, OverflowError) as e:
        yield jsonschema.ValidationError(str(e))


def byte_limit(validator, limit, value, schema):
    if isinstance(value, str) and len(value.encode('utf-8')) > limit:
        yield jsonschema.ValidationError('UTF8_BYTES')


def canonical_bytes(validator, limit, value, schema):
    if len(canonical(value)) > limit:
        yield jsonschema.ValidationError('CANONICAL_BYTES')


def embedded(validator, name, value, schema):
    if not isinstance(value, str):
        return
    try:
        parsed = strict(value.encode())
        if canonical(parsed).decode() != value:
            raise ValueError('EMBEDDED_CANONICAL_BYTES')
    except (ValueError, UnicodeError) as e:
        yield jsonschema.ValidationError(str(e))
        return
    yield from validator.descend(parsed, {'$ref': '#/$defs/' + name})


Validator = jsonschema.validators.extend(jsonschema.Draft202012Validator, {
    'pattern': whole_pattern,
    'x-scalar': scalar,
    'x-utf8-maxBytes': byte_limit,
    'x-canonical-maxBytes': canonical_bytes,
    'x-canonicalSchema': embedded,
})
