def clamp(val: float, min_val: float, max_val: float) -> float:
    """Clamps val within [min_val, max_val]."""
    return max(min_val, min(max_val, val))

def lerp(start: float, end: float, factor: float) -> float:
    """Linear interpolation between start and end."""
    return start + (end - start) * factor

def safe_div(num: float, den: float, default: float = 0.0) -> float:
    """Safely divides num by den, returning default on ZeroDivisionError."""
    return num / den if den != 0 else default
