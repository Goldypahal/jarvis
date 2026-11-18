def format_name(s):
    return s.strip().title()

def sanitize_input(s):
    return s.replace('<', '').replace('>', '')

def legacy_obsolete_hash(s):
    return hash(s)
