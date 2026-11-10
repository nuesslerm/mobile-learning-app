"""GIF writing animations -> ordered stroke paths, in the letter's own bbox.

Where the strokes come from, and where they do not. The animations give the
SHAPE and the ORDER honestly: the frames show the letter being laid down, so
walking the skeleton in the order the ink appears follows the line a hand
follows. They do not give the pen LIFTS. Every frame draws, so there is no
pen-up frame to find, and a lift usually lands on ink that is already on the
page -- அ's vertical crosses its own crossbar -- so "the new ink does not touch
anything" cannot see it. Three detectors were tried against this (touching ink,
distance from the previous frame's far end, whether a frame joins the drawing
at its own end) and each one broke a different letter: அ as one stroke, ம as
four, ய as five.

So the COUNT per letter is authored, in COUNTS below, from the letter's shape
and from watching the animations frame by frame. The count then says where to
cut, and the cut points are still measured: the walk records every step where
it had to go back over ink it had already drawn, and those retraces are where
a hand lifts instead. The longest retrace that leaves two real strokes behind
becomes the break, and the retraced points are dropped -- nobody draws them.
A letter with no retrace long enough is cut where the path turns back on itself
hardest, which is the other place the pen comes up.
"""
from collections import defaultdict, deque
from PIL import Image, ImageSequence
import numpy as np, json, sys

COUNTS = {
    # Stroke counts and the motion of each stroke, from a Tamil writer's
    # description of how the letters are formed. The geometry below follows
    # them; nothing here is inferred from the animations, which show a painting
    # order rather than a pen and cannot say where a hand lifts.
    'alet': 2,      # curl + bowl + crossbar, then the stem down through it
    'aalet': 3,     # அ's two, then the loop under the stem
    'ilet': 1,      # loop, arch, down, across, hook up
    'iilet': 4,     # stem, bar into the right stem, then the two dots
    'ulet': 1,      # loop into the bowl and flat across
    'uulet': 2,     # உ, then ள alongside
    'elet': 2,      # loop up the stem and right along the bar, then the right stem
    'eelet': 3,     # எ's two, then the diagonal tail
    'ailet': 1,     # crest, inner twist, baseline, flick up
    'olet': 1,      # loop, down, back up through the middle, bowl across
    'oolet': 1,     # ஒ, closing into a knot
    'aulet': 2,     # ஒ, then ள alongside
    'ahlet': 3,     # three taps
    'ka': 2,        # stem, then bar + loop + kick
    'nga': 2,       # inverted U, then the foot
    'ca': 2,        # stem, then bar + right side + baseline back
    'nja': 2,       # head, then the connector round the bottom
    't8a': 1,       # down and right
    'n3a': 1,       # three loops, baseline, up
    'tha': 2,       # stem, then bar + inward curve + tail
    'nda': 2,       # stem, then bar + hook + sweep
    'pa': 1,        # down, across, up
    'ma': 2,        # the cup, then the crossbar
    'ya': 1,        # two open curves, baseline, up
    'ra': 1,        # down, across, down
    'la': 1,        # loop, two crests, down
    'va': 1,        # loop, arch, baseline, up
    'zha': 2,       # the cup, then the tail under it
    'l9a': 1,       # loop, arch, belly, up
    'r6a': 1,       # zigzag
    'n2a': 1,       # loop, two arches, baseline, up
    # The க series, which UPenn animates too. Left whole on purpose: it is
    # there so compose-uyirmei-strokes.py can cut each animation at the
    # boundaries the font shapes the combination into and keep the vowel
    # sign's own marks. க's strokes come from 'ka' above, already split.
    # 0 means "no target": leave the trails as the skeleton gives them. Forcing
    # these into one stroke made the joins double back, and compose kept the
    # doubling in the vowel sign it cut out -- மொ's ா went up and back down the
    # same line.
    'kaa': 0, 'ki': 0, 'kii': 0, 'ku': 0, 'kuu': 0,
    'ke': 0, 'kee': 0, 'ko': 0, 'koo': 0, 'kau': 0,
}
def components(mask_or_pix):
    """Connected pieces, of a boolean mask or a list of pixels."""
    import numpy as _np
    if hasattr(mask_or_pix, 'shape'):
        pix = [(int(y), int(x)) for y, x in _np.argwhere(mask_or_pix)]
    else:
        pix = list(mask_or_pix)
    S = set(pix); seen = set(); out = []
    for p in pix:
        if p in seen: continue
        grp = []; q = deque([p]); seen.add(p)
        while q:
            c = q.popleft(); grp.append(c)
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    n = (c[0]+dy, c[1]+dx)
                    if n in S and n not in seen:
                        seen.add(n); q.append(n)
        out.append(grp)
    return out


NB8 = [(-1,-1),(-1,0),(-1,1),(0,-1),(0,1),(1,-1),(1,0),(1,1)]


def build(pix, spur=12):
    S = set(pix)
    def nbrs(p): return [(p[0]+dy, p[1]+dx) for dy,dx in NB8 if (p[0]+dy,p[1]+dx) in S]
    special = {p for p in S if len(nbrs(p)) != 2}
    node_of = {}; nodes = []
    for p in special:
        if p in node_of: continue
        grp=[]; q=deque([p]); node_of[p]=len(nodes)
        while q:
            c=q.popleft(); grp.append(c)
            for n in nbrs(c):
                if n in special and n not in node_of:
                    node_of[n]=len(nodes); q.append(n)
        nodes.append(grp)
    if not nodes:                                  # closed loop, cut anywhere
        start = pix[0]; chain=[start]; prev=None; cur=start
        while True:
            nxt=[x for x in nbrs(cur) if x != prev and x not in chain[1:]]
            if not nxt: break
            prev, cur = cur, nxt[0]; chain.append(cur)
            if cur == start: break
        return [[0,0,chain]], [[start]]
    edges=[]; seen=set()
    for ni, grp in enumerate(nodes):
        for s in grp:
            for n in nbrs(s):
                if n in special or (s,n) in seen: continue
                chain=[s,n]; seen.add((s,n)); seen.add((n,s))
                prev, cur = s, n
                while cur not in special:
                    nxt=[x for x in nbrs(cur) if x != prev]
                    if not nxt: break
                    prev, cur = cur, nxt[0]; chain.append(cur)
                    seen.add((prev,cur)); seen.add((cur,prev))
                edges.append([ni, node_of.get(chain[-1], ni), chain])
    # prune hairs hanging off a junction: thinning noise, not strokes
    changed = True
    while changed:
        changed = False
        deg = defaultdict(int)
        for a,b,_ in edges: deg[a]+=1; deg[b]+=1
        for e in list(edges):
            a,b,chain = e
            if len(chain) < spur and (deg[a]==1 or deg[b]==1) and len(edges)>1:
                edges.remove(e); changed=True; break
    return edges, nodes

def node_xy(nodes, i):
    grp = nodes[i]
    return (sum(p[0] for p in grp)/len(grp), sum(p[1] for p in grp)/len(grp))

def duplicate_shortest(edges, want):
    """Make the shape coverable by `want` trails, retracing as little as possible.

    A shape with 2k loose ends needs k trails to cover without repeating a
    line. Below that the pen has to go back over something -- ர's three arms
    meet at one point, so one arm is always drawn twice however it is written.
    Which arm is the whole question: duplicating the SHORTEST edge between two
    loose ends costs the least ink, and the walk then covers everything in one
    pass. The old code joined trails after the fact and ended up retracing
    whichever arm happened to be left over, which on ர was the longest one.
    """
    from collections import defaultdict as _dd
    edges = [list(e) for e in edges]
    while True:
        deg = _dd(int)
        for a, b, _chain in edges:
            deg[a] += 1
            deg[b] += 1
        odd = [n for n in deg if deg[n] % 2 == 1]
        trails_needed = max(1, len(odd) // 2)
        if trails_needed <= want:
            return edges
        both_odd = [e for e in edges if e[0] in odd and e[1] in odd and e[0] != e[1]]
        if not both_odd:
            return edges
        shortest = min(both_odd, key=lambda e: len(e[2]))
        edges.append([shortest[0], shortest[1], list(shortest[2])])


def trails(edges, nodes, time_of):
    """Cover the graph with as few trails as its shape allows.

    Hierholzer, not a greedy walk: walk until stuck, then splice in the loops
    left hanging off nodes that still have unused edges. A greedy walk strands
    edges behind it -- it would finish ய's sweep and leave the crest as a second
    stroke even though one pen path covers both.
    """
    adj = defaultdict(list)
    for i, (a, b, _chain) in enumerate(edges):
        adj[a].append((b, i))
        if b != a:
            adj[b].append((a, i))

    def inked(i):
        return min(time_of.get(p, 1 << 30) for p in edges[i][2])

    def oriented(i, frm):
        chain = list(edges[i][2])
        ay, ax = node_xy(nodes, frm)
        near_start = (chain[0][0] - ay) ** 2 + (chain[0][1] - ax) ** 2
        near_end = (chain[-1][0] - ay) ** 2 + (chain[-1][1] - ax) ** 2
        return chain if near_start <= near_end else chain[::-1]

    used = set()

    def walk(start):
        """Node-by-node path from `start` until nothing unused leads onward."""
        route = [start]
        cur = start
        while True:
            options = [(b, i) for b, i in adj[cur] if i not in used]
            if not options:
                return route
            b, i = min(options, key=lambda o: inked(o[1]))
            used.add(i)
            route.append((i, cur, b))
            cur = b

    def splice(route):
        """Fold every loop still hanging off the route back into it."""
        changed = True
        while changed:
            changed = False
            for position in range(len(route)):
                node = route[0] if position == 0 else route[position][2]
                if not any(i not in used for _, i in adj[node]):
                    continue
                detour = walk(node)
                if len(detour) < 2:
                    continue
                # Only a detour that comes back where it started can be folded
                # in. An open one leaves the pen somewhere else, and splicing
                # it drew a straight line across the letter to rejoin -- the
                # diagonal that ran through க and ந.
                if detour[-1][2] != node:
                    for step in detour[1:]:
                        used.discard(step[0])
                    continue
                route = route[: position + 1] + detour[1:] + route[position + 1 :]
                changed = True
                break
        return route

    def points(route):
        if len(route) < 2:
            return []
        path = []
        for step in route[1:]:
            i, frm, _to = step
            chain = list(edges[i][2])
            if path:
                # Orient against the pen's actual last point, not the node's
                # centre: a junction is a clump several pixels wide, and
                # choosing by its centre flipped chains the wrong way round,
                # which drew a straight chord across the letter to get back.
                last = path[-1]
                head = (chain[0][0] - last[0]) ** 2 + (chain[0][1] - last[1]) ** 2
                tail = (chain[-1][0] - last[0]) ** 2 + (chain[-1][1] - last[1]) ** 2
                if tail < head:
                    chain.reverse()
                path.extend(chain[1:] if chain[0] == last else chain)
            else:
                path.extend(oriented(i, frm))
        return path

    order = sorted(adj, key=lambda n: min(inked(i) for _, i in adj[n]))
    odd = [n for n in order if len(adj[n]) % 2 == 1]
    out = []
    for node in odd + order:
        while any(i not in used for _, i in adj[node]):
            route = splice(walk(node))
            path = points(route)
            if len(path) >= 2:
                out.append(path)
    return out


def _ends(p): return (p[0], p[-1])

def _close(a, b, tol=3):
    return abs(a[0]-b[0]) <= tol and abs(a[1]-b[1]) <= tol

def merge_to(strokes, want, tol=6):
    """Join trails that meet end-to-end: the pen simply carries on.

    Each trail already covers every piece once, so gluing two of them at a
    shared point is still one stroke that never redraws a line.
    """
    strokes = [list(s) for s in strokes]
    while len(strokes) > want:
        best = None
        for i in range(len(strokes)):
            for j in range(len(strokes)):
                if i == j: continue
                for rev_i in (False, True):
                    for rev_j in (False, True):
                        a = strokes[i][::-1] if rev_i else strokes[i]
                        b = strokes[j][::-1] if rev_j else strokes[j]
                        if _close(a[-1], b[0], tol):
                            score = (-min(len(a), len(b)),)
                            if best is None or score < best[0]:
                                best = (score, i, j, rev_i, rev_j)
        if best is None: break
        _, i, j, ri, rj = best
        a = strokes[i][::-1] if ri else strokes[i]
        b = strokes[j][::-1] if rj else strokes[j]
        joined = a + b[1:]
        for k in sorted((i, j), reverse=True): strokes.pop(k)
        strokes.append(joined)
    return strokes

def connect_to(strokes, want, skeleton_pix, span, max_ratio=0.18):
    """Join trails across a short walk along ink that is already drawn.

    A letter whose skeleton has more than two loose ends cannot be covered by
    one trail without going over something twice -- and a hand writing ர or ய
    does exactly that, brushing back along a few millimetres of its own line at
    the crossing. The join is allowed only if that walk is short.
    """
    from collections import deque
    S = set(skeleton_pix)
    def route(a, b):
        if a == b: return [a]
        prev = {a: None}; q = deque([a])
        while q:
            c = q.popleft()
            if c == b: break
            for dy, dx in NB8:
                n = (c[0]+dy, c[1]+dx)
                if n in S and n not in prev:
                    prev[n] = c; q.append(n)
        if b not in prev: return None
        path = []; c = b
        while c is not None: path.append(c); c = prev[c]
        return path[::-1]

    strokes = [list(s) for s in strokes]
    limit = max_ratio * span
    while len(strokes) > want:
        best = None
        for i in range(len(strokes)):
            for j in range(len(strokes)):
                if i == j: continue
                for ri in (False, True):
                    for rj in (False, True):
                        a = strokes[i][::-1] if ri else strokes[i]
                        b = strokes[j][::-1] if rj else strokes[j]
                        r = route(a[-1], b[0])
                        if r is None or len(r) > limit: continue
                        if best is None or len(r) < best[0]:
                            best = (len(r), i, j, ri, rj, r)
        if best is None: break
        _, i, j, ri, rj, r = best
        a = strokes[i][::-1] if ri else strokes[i]
        b = strokes[j][::-1] if rj else strokes[j]
        joined = a + r[1:-1] + b
        for k in sorted((i, j), reverse=True): strokes.pop(k)
        strokes.append(joined)
    return strokes

def split_to(strokes, want):
    """Too few trails: cut the longest where the path turns hardest."""
    strokes = [list(s) for s in strokes]
    while len(strokes) < want:
        idx = max(range(len(strokes)), key=lambda i: len(strokes[i]))
        s = strokes[idx]
        if len(s) < 12: break
        lo, hi = int(0.2*len(s)), int(0.8*len(s))
        if hi <= lo: break
        def turn(i):
            a,b,c = s[max(0,i-6)], s[i], s[min(len(s)-1,i+6)]
            v1=(b[0]-a[0], b[1]-a[1]); v2=(c[0]-b[0], c[1]-b[1])
            m1=(v1[0]**2+v1[1]**2)**.5 or 1; m2=(v2[0]**2+v2[1]**2)**.5 or 1
            return (v1[0]*v2[0]+v1[1]*v2[1])/(m1*m2)
        cut = min(range(lo,hi), key=turn)
        strokes.pop(idx)
        strokes.insert(idx, s[:cut+1]); strokes.insert(idx+1, s[cut:])
    return strokes


def masks(path):
    """Ink masks, palette-agnostic: the background is whatever colour covers
    most of the first frame (ya.gif is light-on-dark, and a fixed threshold
    read it as a blank letter)."""
    im = Image.open(path)
    frames = [np.array(f.convert('L')) for f in ImageSequence.Iterator(im)]
    first = frames[0]
    values, counts = np.unique(first, return_counts=True)
    background = int(values[int(np.argmax(counts))])
    return [np.abs(f.astype(int) - background) > 60 for f in frames]

def dilate(m, r=2):
    g = np.zeros_like(m)
    for dy in range(-r, r+1):
        for dx in range(-r, r+1):
            g |= np.roll(np.roll(m, dy, 0), dx, 1)
    return g

def thin(mask):
    """Zhang-Suen."""
    img = mask.astype(np.uint8).copy()
    def nb(i, j):
        return [img[i-1,j], img[i-1,j+1], img[i,j+1], img[i+1,j+1],
                img[i+1,j], img[i+1,j-1], img[i,j-1], img[i-1,j-1]]
    changed = True
    while changed:
        changed = False
        for step in (0, 1):
            doomed = []
            for i in range(1, img.shape[0]-1):
                for j in range(1, img.shape[1]-1):
                    if img[i,j] != 1: continue
                    n = nb(i,j); s = sum(n)
                    if s < 2 or s > 6: continue
                    t = sum((n[k] == 0 and n[(k+1) % 8] == 1) for k in range(8))
                    if t != 1: continue
                    p2,p3,p4,p5,p6,p7,p8,p9 = n
                    if step == 0:
                        if p2*p4*p6 or p4*p6*p8: continue
                    else:
                        if p2*p4*p8 or p2*p6*p8: continue
                    doomed.append((i,j))
            for i,j in doomed: img[i,j] = 0
            if doomed: changed = True
    return img.astype(bool)

def walk_group(members, times):
    """One continuous path over the group's skeleton, in drawing order.

    Prefers the unvisited neighbour drawn earliest. When a branch dead-ends it
    does NOT jump: it retraces along pixels it has already drawn to reach the
    nearest unvisited one, which is what a hand does when a letter crosses its
    own line. Jumping instead drew long chords across the inside of the letter
    -- one stroke, but not the letter's line.
    """
    from collections import deque
    allp = set(members)
    remaining = set(members)
    start = min(members, key=lambda p: (times[p], p[0], p[1]))
    path = [start]; remaining.discard(start)

    def neighbours(p):
        y, x = p
        return [(y+dy, x+dx) for dy in (-1,0,1) for dx in (-1,0,1)
                if (dy or dx) and (y+dy, x+dx) in allp]

    def route_to_unvisited(src):
        """Shortest walk over drawn pixels to any unvisited pixel."""
        prev = {src: None}
        q = deque([src])
        while q:
            cur = q.popleft()
            for nb in neighbours(cur):
                if nb in prev: continue
                prev[nb] = cur
                if nb in remaining:
                    route = [nb]
                    while prev[route[-1]] is not None:
                        route.append(prev[route[-1]])
                    return list(reversed(route))[1:]
                q.append(nb)
        return None

    while remaining:
        cur = path[-1]
        nexts = [p for p in neighbours(cur) if p in remaining]
        if nexts:
            nxt = min(nexts, key=lambda p: (times[p], p[0], p[1]))
            path.append(nxt); remaining.discard(nxt)
            continue
        route = route_to_unvisited(cur)
        if not route: break
        for p in route:
            path.append(p); remaining.discard(p)
    return path

def trace_contours(mask):
    """Moore-neighbour boundary tracing of every region and hole."""
    import numpy as np
    h, w = mask.shape
    padded = np.zeros((h+2, w+2), bool); padded[1:-1,1:-1] = mask
    seen = set(); contours = []
    NB = [(-1,0),(-1,1),(0,1),(1,1),(1,0),(1,-1),(0,-1),(-1,-1)]

    def boundary_of(start, inside):
        contour = [start]
        b = start; prev_dir = 0
        for _ in range(20000):
            found = False
            for k in range(8):
                d = (prev_dir + 5 + k) % 8 if contour else k
                ny, nx = b[0] + NB[d][0], b[1] + NB[d][1]
                if padded[ny, nx] == inside:
                    prev_dir = d; b = (ny, nx); contour.append(b); found = True
                    break
            if not found: break
            if b == start and len(contour) > 2: break
        return contour

    # outer boundaries: scan for an ink pixel whose left neighbour is empty
    for y in range(1, h+1):
        for x in range(1, w+1):
            if not padded[y, x] or padded[y, x-1]: continue
            if (y, x) in seen: continue
            c = boundary_of((y, x), True)
            for p in c: seen.add(p)
            # A small mark traces a short ring; dropping those left ஊ's
            # lengthening stroke with no outline under it at all.
            if len(c) > 4:
                contours.append([(p[0]-1, p[1]-1) for p in c])
    return contours

def extract(path, name):
    ms = masks(path)
    # The union of every frame, not just the last: a frame can drop ink (the
    # animation redraws parts), and normalising to a final frame that is
    # missing a piece put ஊ's second stroke at x = 2.1 -- twice outside its
    # own box.
    final = ms[0].copy()
    for m in ms:
        final |= m
    time = np.full(final.shape, len(ms), dtype=int)
    prev = np.zeros_like(final)
    for i, m in enumerate(ms):
        time[m & ~prev] = i
        prev = prev | m

    want = COUNTS[name]
    ink_y, ink_x = np.nonzero(final)
    y0, y1, x0, x1 = ink_y.min(), ink_y.max(), ink_x.min(), ink_x.max()
    span = max(y1 - y0, x1 - x0) or 1

    # Each stroke is a TRAIL through the letter's skeleton: it may pass through
    # a crossing, but it never draws the same piece twice. That is what makes
    # the strokes look like writing instead of scribbling -- the earlier walk
    # doubled back over ink it had already laid down, which is visible on the
    # pad as a stroke going over itself.
    strokes = []
    skeleton = []
    ink_pieces = [c for c in components(final) if len(c) >= 6]
    # Each detached piece of ink is at least one stroke -- ஊ's ள is its own
    # mark -- so the letter's count is shared out: one per piece, and any left
    # over go to the biggest, which is where a letter's own pen lifts are.
    share = {id(c): 1 for c in ink_pieces}
    if want and ink_pieces:
        biggest = max(ink_pieces, key=len)
        share[id(biggest)] = max(1, want - (len(ink_pieces) - 1))
    for comp in ink_pieces:
        blob = np.zeros_like(final)
        for (y, x) in comp:
            blob[y, x] = True
        pix = [(int(y), int(x)) for y, x in np.argwhere(thin(blob))]
        if len(pix) < 2:
            ys2, xs2 = np.nonzero(blob)
            cy, cx = int(ys2.mean()), int(xs2.mean())
            strokes.append([(cy, cx - 1), (cy, cx + 1)])
            continue
        skeleton += pix
        edges, nodes = build(pix, spur=12)
        if want:
            edges = duplicate_shortest(edges, share[id(comp)])
        here = trails(edges, nodes, {q: int(time[q]) for q in pix})
        # Thinning noise is short, but so is a mark: ஈ's dots and ஃ's are whole
        # pieces of ink in their own right, so every piece keeps at least its
        # longest trail however small that is.
        big = [t for t in here if len(t) >= 6]
        strokes.extend(big or ([max(here, key=len)] if here else []))


    # Too many pieces: join the ones that meet end to end, then -- only if the
    # count still will not come down -- the ones a short walk along ink already
    # drawn can reach. A hand writing ர or ய does brush back over a few
    # millimetres of its own line at the crossing; that is the only overlap
    # allowed, and it is bounded.
    if want:
        if len(strokes) > want:
            strokes = merge_to(strokes, want, tol=6)
        # A hand does brush back over a few millimetres of its own line at a
        # crossing, so trails may be joined that way -- but only that far. The
        # long joins this used to allow drew a second line down ர and ய, which
        # is the doubling back the whole approach exists to avoid. A letter
        # that cannot reach its authored count within the limit stays a stroke
        # over: an extra pen lift reads as writing, a doubled line does not.
        for ratio in (0.08, 0.18, 0.3):
            if len(strokes) <= want:
                break
            strokes = connect_to(strokes, want, skeleton, span, max_ratio=ratio)
        if len(strokes) < want:
            strokes = split_to(strokes, want)
    # Left to right, then top to bottom -- the order Tamil is written in, and
    # the order the letters are described in: அ's curl before its stem, ம's cup
    # before its crossbar, க's stem before the bar and loop. The animation's own
    # frame order cannot be used for this: it paints, and a stroke that merely
    # touches an early piece would sort ahead of the one a hand starts with.
    strokes.sort(key=lambda t: (min(q[1] for q in t), min(q[0] for q in t)))

    if not strokes:
        return None

    def norm(p):
        return [round(float(p[1] - x0) / span, 4), round(float(p[0] - y0) / span, 4)]

    return {
        'frames': len(ms),
        'contours': [[norm((y, x)) for (y, x) in c] for c in trace_contours(final)],
        'wanted': want,
        'strokes': [[norm(p) for p in t] for t in strokes],
        'aspect': [float(x1 - x0) / span, float(y1 - y0) / span],
    }


def main():
    import glob, os
    out = {}
    for path in sorted(glob.glob('gifs/*.gif')):
        name = os.path.basename(path)[:-4]
        if name not in COUNTS:
            print(name, 'SKIP (no authored stroke count)'); continue
        try:
            r = extract(path, name)
        except Exception as e:
            print(name, 'ERR', e); continue
        if r:
            out[name] = r
            flag = 'ok ' if not r['wanted'] or len(r['strokes']) == r['wanted'] else 'BAD'
            print(f"{flag} {name:7s} frames={r['frames']:2d} strokes={len(r['strokes'])}/{r['wanted']} pts={[len(s) for s in r['strokes']]}")
    json.dump(out, open('strokes.json','w'))
    print('wrote strokes.json', len(out))


if __name__ == '__main__':
    main()
