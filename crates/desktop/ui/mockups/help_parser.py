"""Read option tables without assuming which CLI framework produced them."""

import re

PARSER_REVISION = 4
ANSI = re.compile(r"\x1b\][^\x07]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]")
ALIASES = re.compile(r"(?<![\w-])(--?[A-Za-z?][\w.-]*|/\?)(?![\w-])")
META = re.compile(r"^(?:\[?[=<{]|[A-Z][A-Z0-9_-]*(?:\s|$)|(?:arg|cmd|file|path|directory|command|value|option|pattern)(?:\b|=))")


def clean_lines(text):
    for raw in ANSI.sub("", text).replace("\r\n", "\n").splitlines():
        line = raw.expandtabs(4).rstrip()
        if re.match(r"^\s*[╭╰┌└─━┬┴┼]+", line):
            title = re.sub(r"[╭╮╰╯┌┐└┘─━┬┴┼]", "", line).strip()
            yield title + ":" if title else ""
        else:
            yield re.sub(r"\s*[│┃]\s*$", "", re.sub(r"^\s*[│┃] ?", "  ", line))


def option_block(line):
    stripped = line.strip()
    if not re.match(r"^(?:--?[A-Za-z?]|/\?)", stripped):
        return None
    parts = re.split(r"\s{2,}|\s+:\s+", stripped)
    syntax = parts.pop(0)
    # Rich tables may put short names, long names and the metavar in separate columns.
    while parts and (re.match(r"^(?:--?[A-Za-z?]|/\?)", parts[0]) or META.match(parts[0]) and not re.search(r"[a-z]{3}", parts[0])):
        syntax += " " + parts.pop(0)
    if not parts:
        # Compact getopt/docopt tables sometimes use a single space before prose.
        compact = re.match(r"^((?:(?:--?[\w?.-]+|/\?)(?:[= ](?:<[^>]+>|\[[^]]+\]|[A-Z][A-Z0-9_-]*))?[, /|]*)+)\s+([A-Za-z][a-z].*)$", syntax)
        if compact:
            syntax, description = compact.groups()
            return {"syntax": syntax.strip(), "description": description, "descriptionColumn": line.index(description)}
    return {"syntax": syntax, "description": " ".join(parts),
            "descriptionColumn": line.index(parts[0]) if parts else len(line) - len(line.lstrip()) + 2}


def option_details(block, command):
    syntax, description = block["syntax"], block["description"].strip()
    scan_syntax = re.sub(r"--\[no-\]([\w-]+)", r"--\1 / --no-\1", syntax)
    aliases = list(dict.fromkeys(ALIASES.findall(scan_syntax)))
    if syntax == "--":
        aliases = ["--"]
    if not aliases:
        return {}
    metadata_type = re.search(r"\[(boolean|string|number|array|count)\]", description, re.I)
    value_type = metadata_type.group(1).lower() if metadata_type else ""
    remainder = ALIASES.sub("", scan_syntax).strip(" ,/|")
    has_value = bool(re.search(r"[=<{]|\[[^]]+\]|(?:^|\s)(?:[A-Z][A-Z0-9_-]*|arg|cmd|mod|file|path|directory|command|opt|option|value|pattern|string|int\d*|uint\d*|float\d*|duration)(?:\s|$)", remainder))
    if value_type:
        has_value = value_type not in ("boolean", "count")
    is_optional = bool(re.match(r"\[(?:=|<|[A-Za-z])", remainder))
    value_label = re.search(r"<([^>]+)>|\{([^}]+)\}|\b([A-Z][A-Z0-9_-]*)\b", remainder)
    default = re.search(r"\[default:\s*([^]]+)\]|\(default:\s*([^)]*)\)", description, re.I)
    choices = re.search(r"\[choices:\s*([^]]+)\]|\{([^}]+)\}", syntax + " " + description, re.I)
    result = {}
    for alias in aliases:
        suffix = scan_syntax.split(alias, 1)[1]
        join = "none"
        if has_value:
            join = "equals" if alias == "-D" or re.match(r"\[?=", suffix) else "attached" if suffix.startswith("<") and not alias.startswith("--") else "space"
        result[alias] = {
            "syntax": syntax, "description": description,
            "takesValue": has_value and not is_optional, "acceptsValue": has_value,
            "join": join, "sourceCommand": command,
            "valueLabel": next((value for value in value_label.groups() if value), "") if value_label else value_type,
            "valueType": value_type, "variadic": "..." in syntax or "…" in syntax or value_type == "array",
            "defaultValue": next((value for value in default.groups() if value is not None), "") if default else None,
            "choices": next((value for value in choices.groups() if value), "") if choices else "",
        }
    return result


def parse_help(text, command="--help"):
    lines = list(clean_lines(text))
    blocks, commands, positionals, usages = [], {}, [], []
    current, section = None, ""
    for line in lines:
        stripped = line.strip()
        usage = re.match(r"(?i)^(usage|synopsis|사용법):\s*(.*)", stripped)
        if usage:
            usages.append(stripped)
        heading = re.match(r"^(?:available |global |common |optional |required |other |sub|the )?(commands?|subcommands?|options?|flags?|arguments?|positional arguments?)(?: are)?(?:\s*\([^)]*\))?:?$", stripped, re.I)
        if re.match(r"(?i)^these are common .*commands", stripped):
            section = "commands"
        if stripped.endswith(":" ) and not heading and not usage and "command" not in section:
            section = ""
        if heading:
            if current:
                blocks.append(current)
                current = None
            section = heading.group(1).lower()
            continue
        indent = len(line) - len(line.lstrip())
        is_continuation = current and indent >= current["descriptionColumn"]
        block = None if is_continuation else option_block(line)
        if block:
            if current:
                blocks.append(current)
            current = block
            continue
        if current:
            if stripped and (line.startswith(" ") or stripped.startswith("[")):
                current["description"] += " " + stripped
                continue
            blocks.append(current)
            current = None
        if usages and usages[-1].lower().rstrip().endswith(":") and stripped and line.startswith(" "):
            usages[-1] += " " + stripped
            continue
        if "command" in section and stripped:
            match = re.match(r"^([a-zA-Z][\w-]*)(?:[,|]([\w-]+))?(?:\s+[^ ]+)?\s{2,}(.+)$", stripped)
            if match:
                for name in (match.group(1), match.group(2)):
                    if name:
                        commands[name] = {"name": name, "description": match.group(3).strip()}
        elif "argument" in section and stripped:
            parts = re.split(r"\s{2,}", stripped, maxsplit=1)
            if len(parts) == 2 and re.fullmatch(r"[\w<>\[\]{},.-]+", parts[0]):
                positionals.append({"name": parts[0], "description": parts[1]})
                for name in re.findall(r"[A-Za-z][\w-]*", parts[0]) if parts[0].startswith("{") else []:
                    commands[name] = {"name": name, "description": parts[1]}
    if current:
        blocks.append(current)
    options = {}
    for block in blocks:
        for alias, details in option_details(block, command).items():
            if alias not in options or len(details["description"]) > len(options[alias]["description"]):
                options[alias] = details
    output_format = "rich" if re.search(r"[╭│╰]", text) else "typed" if re.search(r"\[(?:boolean|string|number|array)\]", text) else "option-table"
    return {"options": options, "commands": commands, "positionals": positionals,
            "usage": "\n".join(usages[:4]), "format": output_format,
            "isHelp": bool(usages or options and ("--help" in options or "-h" in options or len(options) > 1))}
