"""Every opcode of every encoding family, named by the reference disassembler.

The decoder's tables hold only the instructions a fixture happened to exercise (D139), so a title
using any other one stops on an opcode no table names. This builds, for every family in
`crates/orbistoun-shader/data/encodings.toml` and every value of its opcode field, instruction words
whose source fields name plain registers - never the literal code, so no word asks for a dword after
it - and has LLVM's AMDGPU disassembler (the same reference the fixtures use, in the `silkeh/clang:21`
container) name them. A word it cannot decode names nothing; a name is only ever the reference's.

    python tools/shader-fixtures/sweep.py OUT_DIR

writes `OUT_DIR/sweep-words.txt` (the words, one instruction a line, as llvm-mc reads them),
`OUT_DIR/sweep.out` (what the disassembler printed) and `OUT_DIR/sweep.tsv` (family, opcode, name,
encoding bytes, the instruction as printed: two samples an opcode where two decode).
"""

import os
import random
import re
import subprocess
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
ENCODINGS = os.path.join(REPO, 'crates', 'orbistoun-shader', 'data', 'encodings.toml')
IMAGE = 'silkeh/clang:21'
MCPU = ['-arch=amdgcn', '-mcpu=gfx1013', '-mattr=+wavefrontsize64']

# A source field's value: a plain register, never the literal code (0xFF in 8 bits, 0xFF in the
# low byte of 9) nor a code that needs a following dword.
SCALAR_SOURCES = [0x02, 0x05, 0x0c, 0x1a]
VECTOR_SOURCES = [0x101, 0x104, 0x10a, 0x113]


def field_mask(shift, width):
    return ((1 << width) - 1) << shift


def words_for(family, opcode, sample, rng):
    """Two words (or one) of `family` at `opcode`, the `sample`-th variant."""
    mask = int(family['mask'], 16)
    value = int(family['value'], 16)
    op = family['opcode']
    first = value | (opcode << op['shift'])
    fixed = mask | field_mask(op['shift'], op['width'])
    # Sample 0 clears every bit nothing fixes: the opcode's plainest form, whose name is the one
    # kept. Sample 1 varies only the low byte, a destination register in most families (not an
    # image's, where it is the dimension and so the name). Samples 2 and 3 vary every free bit.
    # Each source field is then set to a plain register.
    two = family['width_bytes'] == 8
    if sample in (0, 4, 5, 6, 7, 8, 9):
        second = 0 if two else None
    elif sample == 1:
        if family['name'] != 'MIMG':
            first |= (3 + rng.randrange(5)) & ~fixed & 0xFF
        second = 0 if two else None
    else:
        first |= rng.getrandbits(32) & ~fixed & 0xFFFFFFFF
        second = rng.getrandbits(32) if two else None
    # Source patterns for samples 6 to 9, by field order: scalar, scalar then an inline constant
    # with no third source, vector then an inline constant, and vectors with no third source - an
    # instruction of two sources refuses a third that is not zero (`v_writelane_b32 v19, s3, 3`).
    patterns = {
        6: [0x01, 0x02, 0x03],
        7: [0x03, 0x83, 0x00],
        8: [0x101, 0x82, 0x00],
        9: [0x101, 0x102, 0x00],
    }
    for index, operand in enumerate(family.get('literal_operands', [])):
        shift, width, word = operand['shift'], operand['width'], operand.get('word', 0)
        # A nine-bit source takes a vector register in even samples and a scalar one in odd ones:
        # some instructions accept only one (`v_writelane_b32` reads a scalar).
        pool = VECTOR_SOURCES if width == 9 and sample % 2 == 0 else SCALAR_SOURCES
        code = pool[(sample + shift) % len(pool)] & ((1 << width) - 1)
        if sample in patterns and index < 3:
            code = patterns[sample][index] & ((1 << width) - 1)
        if word == 0:
            first = (first & ~field_mask(shift, width)) | (code << shift)
        elif second is not None:
            second = (second & ~field_mask(shift, width)) | (code << shift)
    extension = family.get('opcode_extension')
    if extension and second is not None:
        second &= ~field_mask(extension['shift'], extension['width'])
    out = first.to_bytes(4, 'little')
    if second is not None:
        out += second.to_bytes(4, 'little')
    return out


def classify(families, encoding):
    """The family and opcode the decoder reads from `encoding`, most specific mask first."""
    first = int.from_bytes(encoding[:4], 'little')
    second = int.from_bytes(encoding[4:8], 'little') if len(encoding) >= 8 else 0
    for family in sorted(families, key=lambda f: -bin(int(f['mask'], 16)).count('1')):
        if first & int(family['mask'], 16) != int(family['value'], 16):
            continue
        op = family['opcode']
        if op['width'] == 0:
            return None
        opcode = (first >> op['shift']) & ((1 << op['width']) - 1)
        extension = family.get('opcode_extension')
        if extension:
            opcode |= ((second >> extension['shift']) & 1) << op['width']
        return family['name'], opcode
    return None


def main():
    out_dir = sys.argv[1]
    os.makedirs(out_dir, exist_ok=True)
    families = tomllib.load(open(ENCODINGS, 'rb'))['encoding']
    rng = random.Random(0x5EED)
    planned = {}
    lines = []
    for family in families:
        op = family['opcode']
        if op['width'] == 0:
            continue
        extensions = [0]
        if family.get('opcode_extension'):
            extensions = [0, 1]
        for opcode in range(1 << op['width']):
            for extension in extensions:
                for sample in range(10):
                    raw = words_for(family, opcode, sample, rng)
                    if extension:
                        bit = family['opcode_extension']
                        second = int.from_bytes(raw[4:8], 'little') | (1 << bit['shift'])
                        raw = raw[:4] + second.to_bytes(4, 'little')
                    key = raw.hex()
                    full = opcode | (extension << op['width'])
                    planned[key] = (family['name'], full, sample)
                    lines.append(','.join(f'0x{b:02x}' for b in raw))
    words = os.path.join(out_dir, 'sweep-words.txt')
    open(words, 'w', newline='\n').write('\n'.join(lines) + '\n')
    mounted = os.path.abspath(out_dir)
    printed = subprocess.run(
        ['docker', 'run', '--rm', '-v', f'{mounted}:/w', '-w', '/w', IMAGE, 'llvm-mc', *MCPU,
         '-disassemble', '-show-encoding', 'sweep-words.txt'],
        capture_output=True, text=True, env={**os.environ, 'MSYS_NO_PATHCONV': '1'},
    )
    open(os.path.join(out_dir, 'sweep.out'), 'w', newline='\n').write(printed.stdout + printed.stderr)
    line = re.compile(r'^\s*(\S.*?)\s*;\s*encoding:\s*\[([^\]]*)\]')
    found = {}
    for text in printed.stdout.splitlines():
        m = line.match(text)
        if not m:
            continue
        encoding = bytes(int(b, 16) for b in m.group(2).split(','))
        # The family and opcode are read from the bytes the disassembler printed, by the masks the
        # decoder uses: it may print another encoding of the word it was given.
        classified = classify(families, encoding)
        if classified is None:
            continue
        family, opcode = classified
        planned_sample = next((planned[encoding[:n].hex()][2] for n in (8, 4)
                               if encoding[:n].hex() in planned), 9)
        sample = planned_sample
        name = m.group(1).split()[0]
        found.setdefault((family, opcode), []).append((name, encoding.hex(), m.group(1), sample))
    for samples in found.values():
        samples.sort(key=lambda sample: sample[3])
    rows = []
    for (family, opcode), samples in sorted(found.items()):
        # The plainest form's name where it decoded (the first sample planned), else the most common.
        names = [name for name, _, _, _ in samples]
        chosen = samples[0][0] if samples[0][3] == 0 else max(set(names), key=names.count)
        kept = [sample for sample in samples if sample[0] == chosen][:2]
        for name, encoding, text, _ in kept:
            rows.append(f'{family}\t{opcode}\t{name}\t{encoding}\t{text}\t{len(kept)}')
    open(os.path.join(out_dir, 'sweep.tsv'), 'w', newline='\n').write('\n'.join(rows) + '\n')
    print(f'{len(lines)} words, {len(found)} family opcodes named')


if __name__ == '__main__':
    main()
