"""Consonant + vowel sign -> ordered stroke paths for the uyirmei grid.

UPenn animates the 31 base letters and, of the 216 combinations, only the க
series. The rest are composed here, and the composition uses nothing invented:

  * the consonant's strokes are its own writing animation
  * the sign's strokes are the same sign as written in the க series, which is
    where the animations already show it being drawn
  * the ORDER is the order the க-series animation draws those pieces in --
    கொ is written ெ, then க, then ா, and that order is the sign's, not க's
  * WHERE each piece sits comes from shaping the real string with HarfBuzz, so
    it is the font's own layout, not a guess about which side a sign goes on

The pieces only pair up because the animation draws each glyph as one stroke in
visual order, so stroke i of the க-series animation is glyph i of the shaped
string. A combination the font draws as a single ligature (every -ii, -u and -uu
form, plus டி and லி) has no consonant glyph to substitute into and is skipped,
as is -ai, which UPenn does not animate at all.

The outline written out is traced from the font's own rendering of the
combination, NOT from the composed strokes -- so the coverage and on-glyph
gates in extract-tamil-glyphs.ts measure the composition against an independent
drawing of the letter, and refuse it when the pieces do not land.

Needs `pillow` (built with libraqm, or Tamil reordering does not happen) and
`uharfbuzz`: pip3 install pillow uharfbuzz.
"""
import json
import sys
from pathlib import Path

import numpy as np
import uharfbuzz as hb
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).parent))
from importlib import import_module

trace_contours = import_module('extract-upenn-strokes').trace_contours

HERE = Path(__file__).parent
FONT = HERE / '..' / '..' / 'assets' / 'fonts' / 'NotoSansTamil-Regular.ttf'
PX = 200
MARGIN = 60

# The க-series animation for each sign. Which of its strokes belong to க and
# which to the sign is not authored -- it is read off the shaping below, by
# asking which glyph box each stroke sits in.
SERIES = {
    'aa': 'kaa',
    'e': 'ke',
    'ee': 'kee',
    'o': 'ko',
    'oo': 'koo',
    'au': 'kau',
}

CONSONANTS = [
    ('k', 'க', 'ka'),
    ('ng', 'ங', 'nga'),
    ('c', 'ச', 'ca'),
    ('nj', 'ஞ', 'nja'),
    ('tt', 'ட', 't8a'),
    ('nn', 'ண', 'n3a'),
    ('t', 'த', 'tha'),
    ('n-dental', 'ந', 'nda'),
    ('p', 'ப', 'pa'),
    ('m', 'ம', 'ma'),
    ('y', 'ய', 'ya'),
    ('r', 'ர', 'ra'),
    ('l', 'ல', 'la'),
    ('v', 'வ', 'va'),
    ('zh', 'ழ', 'zha'),
    ('ll', 'ள', 'l9a'),
    ('rr', 'ற', 'r6a'),
    ('n-alveolar', 'ன', 'n2a'),
]

SIGNS = {
    'aa': 'ா',
    'e': 'ெ',
    'ee': 'ே',
    'o': 'ொ',
    'oo': 'ோ',
    'au': 'ௌ',
}

blob = hb.Blob.from_file_path(str(FONT))
face = hb.Face(blob)
hbfont = hb.Font(face)
hbfont.scale = (PX, PX)
pilfont = ImageFont.truetype(str(FONT), PX)


def glyph_boxes(text):
    """Each shaped glyph's ink box, in the same pixels PIL draws into."""
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()
    hb.shape(hbfont, buf)
    x = 0.0
    out = []
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        ext = hbfont.get_glyph_extents(info.codepoint)
        left = MARGIN + x + pos.x_offset + ext.x_bearing
        top = PX + MARGIN - (pos.y_offset + ext.y_bearing)
        out.append((left, top, float(ext.width), float(-ext.height)))
        x += pos.x_advance
    return out, x


def render(text, advance):
    width = int(advance) + 2 * MARGIN
    height = PX * 2 + 2 * MARGIN
    im = Image.new('L', (width, height), 255)
    ImageDraw.Draw(im).text((MARGIN, PX + MARGIN), text, font=pilfont, fill=0, anchor='ls')
    return np.array(im) < 128


def fit(strokes, box):
    """Map a glyph's strokes into its box, as one piece.

    All of a glyph's strokes share the transform: fitting each to the box on
    its own stretches every one of them to the full glyph, which is how க's
    short first stroke ended up crossing the letter instead of sitting in its
    corner.
    """
    left, top, width, height = box
    xs = [p[0] for s in strokes for p in s]
    ys = [p[1] for s in strokes for p in s]
    x0, x1 = min(xs), max(xs)
    y0, y1 = min(ys), max(ys)
    sx = width / (x1 - x0) if x1 > x0 else 0.0
    sy = height / (y1 - y0) if y1 > y0 else 0.0
    return [
        [(left + (p[0] - x0) * sx, top + (p[1] - y0) * sy) for p in s]
        for s in strokes
    ]


def sign_pieces(sign_key):
    """The vowel sign's own strokes, cut out of the க-series animation.

    The animation draws the whole combination; the font's shaping says which
    part of it is க and which is the sign, so the path is cut where it crosses
    from one glyph's box into another. What comes back is the writing order
    with a placeholder where க's own strokes belong -- so another consonant can
    put its own strokes there, however many it writes.
    """
    text = 'க' + SIGNS[sign_key]
    boxes, advance = glyph_boxes(text)
    mask = render(text, advance)
    ink_y, ink_x = np.nonzero(mask)
    x0, x1 = int(ink_x.min()), int(ink_x.max())
    y0, y1 = int(ink_y.min()), int(ink_y.max())
    span = max(y1 - y0, x1 - x0) or 1
    spans = [((left - x0) / span, (left + width - x0) / span) for left, _t, width, _h in boxes]
    bare, _ = glyph_boxes('க')
    consonant = min(range(len(boxes)), key=lambda i: abs(boxes[i][2] - bare[0][2]))

    def box_of(point):
        return min(range(len(spans)), key=lambda k: 0 if spans[k][0] <= point[0] <= spans[k][1]
                   else min(abs(point[0] - spans[k][0]), abs(point[0] - spans[k][1])))

    pieces = []
    for stroke in SERIES_STROKES[sign_key]:
        run = []; run_box = None
        for point in stroke:
            b = box_of(point)
            if run_box is None or b == run_box:
                run.append(point); run_box = b
            else:
                if len(run) >= 2: pieces.append((run_box, run))
                run = [point]; run_box = b
        if len(run) >= 2: pieces.append((run_box, run))

    # A sign is written as one mark, so its pieces are joined back end to end.
    # Joining, not retracing: the pieces meet where the cut was made.
    def join(runs, tol=0.08, close=True):
        runs = [list(r) for r in runs]
        merged = True
        while merged and len(runs) > 1:
            merged = False
            for i in range(len(runs)):
                for j in range(len(runs)):
                    if i == j: continue
                    for ri in (False, True):
                        for rj in (False, True):
                            a = runs[i][::-1] if ri else runs[i]
                            b = runs[j][::-1] if rj else runs[j]
                            if abs(a[-1][0]-b[0][0]) <= tol and abs(a[-1][1]-b[0][1]) <= tol:
                                joined = a + b[1:]
                                for k in sorted((i, j), reverse=True): runs.pop(k)
                                runs.append(joined); merged = True
                                break
                        if merged: break
                    if merged: break
                if merged: break
        if not close:
            return runs
        # Still in pieces: the mark's own skeleton has more loose ends than one
        # trail can cover, so the nearest ends are joined directly. ா is one
        # mark whatever its skeleton says, and the gates refuse the join if it
        # leaves the letter.
        while len(runs) > 1:
            best = None
            for i in range(len(runs)):
                for j in range(len(runs)):
                    if i == j: continue
                    for ri in (False, True):
                        for rj in (False, True):
                            a = runs[i][::-1] if ri else runs[i]
                            b = runs[j][::-1] if rj else runs[j]
                            d = (a[-1][0]-b[0][0])**2 + (a[-1][1]-b[0][1])**2
                            if best is None or d < best[0]:
                                best = (d, i, j, ri, rj)
            _, i, j, ri, rj = best
            a = runs[i][::-1] if ri else runs[i]
            b = runs[j][::-1] if rj else runs[j]
            joined = a + b
            for k in sorted((i, j), reverse=True): runs.pop(k)
            runs.append(joined)
        return runs

    by_tag = {}
    order = []
    for b, run in pieces:
        tag = 'C' if b == consonant else f'sign{b}'
        if tag not in by_tag:
            by_tag[tag] = []; order.append(tag)
        by_tag[tag].append(run)

    out = []
    for tag in order:
        if tag == 'C':
            out.append(('C', by_tag[tag][0]))   # க's own animation supplies these
            continue
        # close=False: pieces that do not meet stay separate strokes. Joining
        # them at their nearest ends drew ா down a line and straight back up
        # it, which is the doubling-back this whole pipeline exists to avoid.
        for run in join(by_tag[tag], close=False):
            out.append((tag, run))
    return out


def compose(consonant_glyph, sign_key, pieces):
    """Place each piece in the box the font shapes it into, in writing order.

    `pieces` is the writing order as (tag, points): 'C' for a stroke of the
    consonant itself, 'sign'/'sign2' for the vowel sign's own marks. The font's
    shaping gives one box per glyph -- the consonant is one box however many
    strokes it takes -- and each piece is fitted into its box.
    """
    text = consonant_glyph + SIGNS[sign_key]
    boxes, advance = glyph_boxes(text)
    order = []
    for tag, _ in pieces:
        if tag not in order: order.append(tag)
    if len(boxes) != len(order):
        return None, f'{len(boxes)} glyphs but {len(order)} pieces to place'
    mask = render(text, advance)
    if not mask.any():
        return None, 'the font drew nothing'
    box_of = {tag: boxes[i] for i, tag in enumerate(order)}

    grouped = {}
    for tag, points in pieces:
        grouped.setdefault(tag, []).append(points)
    fitted = {tag: fit(strokes, box_of[tag]) for tag, strokes in grouped.items()}
    taken = {tag: 0 for tag in grouped}
    placed = []
    for tag, _ in pieces:
        placed.append(fitted[tag][taken[tag]]); taken[tag] += 1

    ink_y, ink_x = np.nonzero(mask)
    y0, y1 = int(ink_y.min()), int(ink_y.max())
    x0, x1 = int(ink_x.min()), int(ink_x.max())
    span = max(y1 - y0, x1 - x0) or 1

    def norm_xy(x, y):
        return [round((x - x0) / span, 4), round((y - y0) / span, 4)]

    contours = trace_contours(mask)
    return {
        'glyph': text,
        'contours': [[norm_xy(x, y) for (y, x) in c] for c in contours],
        'strokes': [[norm_xy(x, y) for (x, y) in s] for s in placed],
        'aspect': [(x1 - x0) / span, (y1 - y0) / span],
        'composed': True,
    }, None


SERIES_STROKES = {}


def main():
    upenn = json.load(open(HERE / 'upenn-strokes.json'))
    for sign_key, series_name in SERIES.items():
        SERIES_STROKES[sign_key] = upenn[series_name]['strokes']
    out = {}
    skipped = []
    for key, glyph, animation in CONSONANTS:
        consonant_strokes = upenn[animation]['strokes']
        for sign_key in SERIES:
            cid = f'uyirmei-{key}-{sign_key}'
            template = sign_pieces(sign_key)
            if not any(tag == 'C' for tag, _ in template):
                skipped.append((cid, f'no part of the {sign_key} animation landed on க'))
                continue
            pieces = []
            for tag, run in template:
                if tag == 'C':
                    pieces.extend(('C', own) for own in consonant_strokes)
                else:
                    pieces.append((tag, run))
            entry, why = compose(glyph, sign_key, pieces)
            if entry is None:
                skipped.append((cid, why))
                continue
            out[cid] = entry
            print(f"{cid:24s} {len(entry['strokes'])} stroke(s), {len(entry['contours'])} contour(s)")
    json.dump(out, open(HERE / 'composed-strokes.json', 'w'))
    print(f"wrote composed-strokes.json: {len(out)} composed, {len(skipped)} skipped")
    for cid, why in skipped:
        print(f"  skipped {cid}: {why}")


if __name__ == '__main__':
    main()
