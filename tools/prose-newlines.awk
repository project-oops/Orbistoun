# String literals that span a line break and continue a sentence on the next.
#
#   awk -f tools/prose-newlines.awk <files>
#
# # What this catches that the continuation guard cannot
#
# A `\` at the end of a line inside a literal is one way to write a sentence across two lines,
# and `bin/orbistoun`'s `prose` step refuses it: `cargo fmt` collapses such a literal and bakes
# the source indentation into the rendered text (D184).
#
# A literal can also just **contain** a newline, with no backslash at all. Then the newline and
# the next line's indentation are in the rendered text from the moment it is written, and no
# formatter has to do anything for the damage to be there. The continuation guard cannot see it,
# and neither can a search for runs of spaces, because the run is not on one line.
#
# Six such messages were found in one session and every one had shipped - a refusal a reader sees
# with twenty-nine spaces through the middle of it, three assertions, a print, and a lint's own
# reason (worklogs 578 and 582).
#
# # What is not damage
#
# Deliberate multi-line output. A generated block or a report with a leading newline means the
# newline; what it does not mean is thirty spaces in the middle of a sentence. So the test is
# narrow: the break happens inside a literal, and the next line is **deep indentation followed by
# a lowercase word** - a sentence carrying on rather than a new line of output.
#
# # How it decides it is inside a literal
#
# A scan toggling on each unescaped quote. That needs no parser because the tree has no
# line-continued literals left - the other guard sees to that - so a line with an odd number of
# quotes opens or closes exactly one. Raw strings are skipped by name rather than half-handled:
# a miss there is a decision instead of an accident.

function strip(line,   out, i, c, n) {
    # Escaped characters cannot open or close anything, so drop each escape and what follows.
    out = ""
    n = length(line)
    for (i = 1; i <= n; i++) {
        c = substr(line, i, 1)
        if (c == "\\") { i++; continue }
        out = out c
    }
    return out
}

BEGIN { inside = 0; found = 0 }

FNR == 1 { inside = 0; skip = 0 }

{
    raw = $0
    if (raw ~ /r"/ || raw ~ /r#"/) { skip = 1 }
    if (skip) next
    if (!inside && raw ~ /^[ \t]*\/\//) next

    was = inside
    clean = raw
    # Character literals first: `'"'` is a quote that opens nothing. A lifetime has no closing
    # quote, so it cannot match this and is left alone.
    gsub(/'[^']'/, "", clean)
    clean = strip(clean)
    n = gsub(/"/, "\"", clean)
    if (n % 2 == 1) inside = !inside

    if (inside && !was) { opened = FNR; text = raw }
    if (was && FNR == opened + 1 && raw ~ /^[ \t]{8,}[a-z]/) {
        printf "%s:%d: a literal carries a newline and this line's indentation into its text\n", \
            FILENAME, opened
        printf "    %s\n    %s\n", text, raw
        found = 1
    }
}

END { exit found }
