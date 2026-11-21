import { afterAll, afterEach, beforeAll, describe, expect, it, type TestContext } from 'vitest';

import { createEditorClient, unwrap, type EditorClient } from '../core/client';
import { createDocumentMirror } from '../core/mirror';
import type { EditorEngine } from '../ports/engine';
import { CORE_REQUEST_NAMES, type EditorError, type EditorEvent, type EventType, type Language, type Range, type Reply, type RequestName, type SelRange, type Step } from '../ports/protocol';

export type SubjectHooks = {
  userTypes?(text: string): Promise<void> | void;
  userSelects?(anchor: number, head?: number): Promise<void> | void;
  /** a new editor instance behind the same engine: the page was created again */
  restart?(): Promise<void> | void;
  /** the next trees need this many calls to `work` before they cover the text, so a test can see a parse that waits (1: at once) */
  slowParse?(calls: number): void;
  /** rpc runners only: the next reply the page sends is lost / the next event it emits is lost / it stops answering until the returned function runs / the channel closes */
  dropNextReply?(): void;
  swallowNextEvent?(): void;
  silentPage?(): () => void;
  closeTransport?(): void;
  /** rpc runners only: the types of the events that crossed the channel so far */
  transportEvents?(): readonly string[];
};
export type Subject = { readonly engine: EditorEngine; dispose(): Promise<void> | void; readonly hooks: SubjectHooks };
export type MakeSubject = (options: { bound: boolean }) => Promise<Subject>;

const TEN = '0123456789';
const replace = (from: number, to: number, text: string, expect?: string): Step => ({ op: 'replace', range: { from, to }, text, ...(expect === undefined ? {} : { expect }) });
const failed = (reply: Reply<unknown>): EditorError => {
  if (reply.ok) throw new Error('the call was expected to fail');
  return reply.error;
};

/** The behaviour every editor behind the typed API must show, whichever engine and transport carry it. */
export type ContractOptions = {
  /** the languages the engine must offer, exactly: an engine that offers a different set fails the lifecycle case */
  readonly languages?: readonly Language[];
};

export function editorContract(name: string, make: MakeSubject, options: ContractOptions = {}): void {
  describe(`the editor contract over ${name}`, () => {
    let subject: Subject;
    let client: EditorClient;
    const cleanups: (() => void)[] = [];
    const extras: Subject[] = [];

    beforeAll(async () => {
      subject = await make({ bound: true });
      client = createEditorClient(subject.engine);
    });
    afterAll(async () => {
      await subject.dispose();
      for (const extra of extras) await extra.dispose();
    });
    afterEach(() => {
      for (const cleanup of cleanups.splice(0)) cleanup();
    });

    const hooks = () => subject.hooks;
    const start = async (text = TEN, selection?: SelRange): Promise<number> => {
      const reply = await client.load(text, selection);
      unwrap(reply);
      return reply.revision as number;
    };
    const record = <T extends EventType>(...types: T[]) => {
      const seen: EditorEvent[] = [];
      cleanups.push(subject.engine.listen(types, (event) => seen.push(event)));
      return seen;
    };
    const snapshot = async () => {
      const reply = await client.getText();
      return { ...unwrap(reply), revision: reply.revision as number };
    };
    const mirrorOf = async () => {
      const mirror = await createDocumentMirror(subject.engine);
      cleanups.push(mirror.stop);
      return mirror;
    };
    const needs = (ctx: TestContext, ...hookNames: (keyof SubjectHooks)[]) => {
      if (hookNames.some((hook) => !hooks()[hook])) ctx.skip();
    };

    describe('lifecycle', () => {
      it('given a bound engine, when hello is called, then it names the api, the instance and what it can do', async () => {
        // Given a bound engine
        // When hello is called
        const hello = unwrap(await client.hello());

        // Then the capabilities are complete
        expect(hello).toMatchObject({ api: 1, ready: true });
        expect(hello.instance.length).toBeGreaterThan(0);
        for (const request of CORE_REQUEST_NAMES) expect(hello.capabilities.requests).toContain(request);
        expect(hello.capabilities.steps).toEqual(expect.arrayContaining(['replace', 'move', 'select']));
        expect(hello.capabilities.events).toEqual(expect.arrayContaining(['ready', 'changed', 'selection', 'focus']));
        expect(hello.capabilities.languages.length).toBeGreaterThan(0);
        if (options.languages) expect(hello.capabilities.languages).toEqual(options.languages);
        expect(hello.capabilities.maxSelections).toBeGreaterThanOrEqual(1);
        expect(hello.capabilities.logDepth).toBeGreaterThan(0);
      });

      it('given an engine that does not announce tree, when tree is called, then it is unsupported naming the request', async (ctx) => {
        const hello = unwrap(await client.hello());
        if (hello.capabilities.requests.includes('tree')) return ctx.skip();
        expect(failed(await client.engine.request('tree', {}))).toMatchObject({ code: 'unsupported', what: 'request:tree' });
      });

      it('given an engine that does not announce complete, when a complete step is applied, then it is unsupported, and hello announces neither the step nor the completion event', async (ctx) => {
        const base = await start();
        const hello = unwrap(await client.hello());
        if (hello.capabilities.steps.includes('complete')) return ctx.skip();
        expect(hello.capabilities.events).not.toContain('completion');
        expect(failed(await client.transact([{ op: 'complete' }]))).toMatchObject({ code: 'unsupported', what: 'step:complete' });
        expect(await snapshot()).toMatchObject({ text: TEN, revision: base });
      });

      it('given a page with no editor yet, when hello and read are called, then it is not ready and read says not-ready', async () => {
        // Given a page before init
        const unbound = await make({ bound: false });
        extras.push(unbound);
        const bare = createEditorClient(unbound.engine);

        // When hello and read are called
        const hello = unwrap(await bare.hello());
        const read = await bare.getText();

        // Then hello is not ready and read is refused
        expect(hello.ready).toBe(false);
        expect(failed(read).code).toBe('not-ready');
      });

      it('given an unknown request, when it is called, then the reply is unsupported', async () => {
        // Given a request name no engine has
        // When it is called
        const reply = await subject.engine.request('nope' as RequestName, undefined as never);

        // Then it is unsupported and names the request
        expect(failed(reply)).toMatchObject({ code: 'unsupported', what: 'request:nope' });
      });

      it('given every step op the engine advertises, when each is sent, then it is accepted, and an op outside the list is unsupported with nothing changed', async () => {
        // Given the advertised ops and a document
        const samples: Record<string, Step> = { replace: replace(0, 0, ''), move: { op: 'move', range: { from: 0, to: 0 }, to: 0 }, select: { op: 'select', anchor: 0 } };
        const base = await start();
        const advertised = unwrap(await client.hello()).capabilities.steps;

        // When each op is sent
        for (const op of advertised) {
          if (op === 'type') continue; // it edits the text and has its own cases in the type group
          expect(samples[op], `an advertised step op has no contract case: ${op}`).toBeDefined();
          expect((await client.transact([samples[op] as Step])).ok).toBe(true);
        }
        const unknown = await client.transact([{ op: 'frobnicate' } as unknown as Step]);

        // Then an unadvertised op is unsupported and nothing changed
        expect(failed(unknown)).toMatchObject({ code: 'unsupported', what: 'step:frobnicate' });
        expect(await snapshot()).toMatchObject({ text: TEN, revision: base });
      });

      it('given a document with edits and undo history, when load runs, then it replaces the text, resets history and stales older revisions', async () => {
        // Given a document with an edit
        const before = await start();
        unwrap(await client.replaceRange({ from: 0, to: 1 }, 'A'));
        const head = (await snapshot()).revision;
        const seen = record('changed');

        // When another text is loaded
        const loaded = await client.load('fresh');

        // Then the text, the revision, the history and the log are reset, and the event says load
        expect(unwrap(loaded)).toMatchObject({ length: 5 });
        expect(loaded.revision).toBe(head + 1);
        expect((await snapshot()).text).toBe('fresh');
        expect(unwrap(await client.undo()).applied).toBe(false);
        expect(failed(await client.replaceRange({ from: 0, to: 0 }, 'x', { at: before })).code).toBe('stale');
        expect(seen).toMatchObject([{ type: 'changed', origin: 'load', from: head, to: head + 1, changes: [{ from: 0, to: 10, insert: 'fresh' }] }]);
      });

      it('given text with CRLF and CR, when it is loaded, then only LF remains and each break counts once', async () => {
        // Given text with all three line endings
        // When it is loaded
        await client.load('a\r\nb\rc');

        // Then read shows LF only
        expect(await snapshot()).toMatchObject({ text: 'a\nb\nc', length: 5 });
      });

      it('given an apply carrying another instance, when it runs, then it is instance-mismatch, and with the right instance it applies', async () => {
        // Given the engine's instance
        await start();
        const { instance } = unwrap(await client.hello());

        // When apply carries another instance, and then the right one
        const wrong = await client.replaceRange({ from: 0, to: 0 }, 'x', { instance: 'someone-else' });
        const right = await client.replaceRange({ from: 0, to: 0 }, 'x', { instance });

        // Then the wrong one is refused and the right one applies
        expect(failed(wrong)).toMatchObject({ code: 'instance-mismatch', expected: instance, actual: 'someone-else' });
        expect(right.ok).toBe(true);
      });
    });

    describe('read', () => {
      it('given a seeded document, when read runs, then text, revision, length and the caret come back', async () => {
        // Given a seeded document
        const revision = await start();

        // When read runs
        const read = await snapshot();

        // Then everything matches the seed
        expect(read).toMatchObject({ text: TEN, length: 10, range: { from: 0, to: 10 }, revision, selection: { ranges: [{ anchor: 0, head: 0 }], main: 0 } });
      });

      it('given a range, when read runs, then exactly its slice returns', async () => {
        await start();
        expect(unwrap(await client.getText({ from: 2, to: 5 }))).toMatchObject({ text: '234', range: { from: 2, to: 5 } });
      });

      it('given a range past the end, when read runs, then it is out-of-range and names the length', async () => {
        await start();
        expect(failed(await client.getText({ from: 5, to: 11 }))).toMatchObject({ code: 'out-of-range', length: 10 });
      });

      it('given a negative, fractional or inverted range, when read runs, then it is invalid and never throws', async () => {
        await start();
        for (const range of [{ from: -1, to: 2 }, { from: 1.5, to: 2 }, { from: 5, to: 2 }]) expect(failed(await client.getText(range)).code).toBe('invalid');
      });
    });

    describe('apply', () => {
      it('given a document, when replace, insert and delete are applied together, then text, revision, changes and landed are exact and one changed event arrives', async () => {
        // Given a seeded document and a listener
        const base = await start();
        const seen = record('changed');

        // When three sites change in one batch
        const reply = await client.transact([replace(2, 4, 'XYZ'), replace(6, 6, '-'), replace(8, 9, '')]);

        // Then text, revision, changes and landed are exact
        expect(unwrap(reply)).toMatchObject({
          changes: [{ from: 2, to: 4, insert: 'XYZ' }, { from: 6, to: 6, insert: '-' }, { from: 8, to: 9, insert: '' }],
          landed: [{ from: 2, to: 5 }, { from: 7, to: 8 }, { from: 10, to: 10 }],
        });
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect((await snapshot()).text).toBe('01XYZ45-679');
        expect(seen).toMatchObject([{ type: 'changed', from: base, to: base + 1, origin: 'api' }]);
      });

      it('given two sites in one batch, when applied, then both use the coordinates of the base revision and it is one revision', async () => {
        const base = await start();
        const reply = await client.transact([replace(0, 2, 'ab'), replace(5, 7, 'cd')]);
        expect((await snapshot()).text).toBe('ab234cd789');
        expect(reply).toMatchObject({ revision: base + 1 });
      });

      it('given two inserts at one position and an insert at the start of a replaced range, when applied, then step order is kept and the insert goes before', async () => {
        await start();
        unwrap(await client.transact([replace(3, 3, 'X'), replace(3, 3, 'Y')]));
        expect((await snapshot()).text).toBe('012XY3456789');
        await start();
        unwrap(await client.transact([replace(5, 7, 'ZZ'), replace(5, 5, 'I')]));
        expect((await snapshot()).text).toBe('01234IZZ789');
      });

      it('given overlapping, out-of-range, unknown and mismatched steps, when applied, then each is refused and nothing changes or fires', async () => {
        // Given a document and a listener
        const base = await start();
        const seen = record('changed');

        // When bad batches are applied
        const overlap = await client.transact([replace(2, 6, 'a'), replace(4, 8, 'b')]);
        const outOfRange = await client.transact([replace(0, 1, 'x'), replace(5, 99, 'y')]);
        const unknown = await client.transact([{ op: 'frobnicate' } as unknown as Step]);
        const mismatch = await client.transact([replace(2, 4, 'x', 'zz')]);

        // Then each is refused with its own code and nothing changed
        expect(failed(overlap)).toMatchObject({ code: 'overlap', steps: [0, 1] });
        expect(failed(outOfRange)).toMatchObject({ code: 'out-of-range', step: 1, length: 10 });
        expect(failed(unknown).code).toBe('unsupported');
        expect(failed(mismatch)).toMatchObject({ code: 'text-mismatch', step: 0, expected: 'zz', actual: '23' });
        expect(await snapshot()).toMatchObject({ text: TEN, revision: base });
        expect(seen).toEqual([]);
        expect(unwrap(await client.undo()).applied).toBe(false);
      });

      it('given moves forward and backward, when applied, then text and landed are right', async () => {
        await start();
        const forward = await client.moveRange({ from: 1, to: 3 }, 7);
        expect((await snapshot()).text).toBe('0345612789');
        expect(unwrap(forward).landed).toEqual([{ from: 5, to: 7 }]);
        await start();
        const backward = await client.moveRange({ from: 7, to: 9 }, 1);
        expect((await snapshot()).text).toBe('0781234569');
        expect(unwrap(backward).landed).toEqual([{ from: 1, to: 3 }]);
      });

      it('given moves that change nothing, when applied, then the revision stays, no event fires and no undo step exists; a destination inside the range is invalid', async () => {
        // Given a document and a listener
        const base = await start();
        const seen = record('changed');

        // When moves that change nothing are applied
        for (const [range, to] of [[{ from: 3, to: 5 }, 3], [{ from: 3, to: 5 }, 5], [{ from: 4, to: 4 }, 8]] as const) expect((await client.moveRange(range, to)).ok).toBe(true);
        const inside = await client.moveRange({ from: 3, to: 6 }, 4);

        // Then nothing happened
        expect(failed(inside)).toMatchObject({ code: 'invalid' });
        expect(await snapshot()).toMatchObject({ text: TEN, revision: base });
        expect(seen).toEqual([]);
        expect(unwrap(await client.undo()).applied).toBe(false);
      });

      it('given an empty replace with empty text, when applied, then it is a no-op', async () => {
        const base = await start();
        const reply = await client.replaceRange({ from: 4, to: 4 }, '');
        expect(reply).toMatchObject({ ok: true, revision: base, base });
        expect((await snapshot()).revision).toBe(base);
      });

      it('given a label, when applied, then the changed event carries it, and an illegal label is invalid', async () => {
        await start();
        const seen = record('changed');
        unwrap(await client.replaceRange({ from: 0, to: 1 }, 'x', { label: 'indent' }));
        expect(seen).toMatchObject([{ label: 'indent' }]);
        expect(failed(await client.replaceRange({ from: 0, to: 1 }, 'y', { label: 'Bad Label' })).code).toBe('invalid');
      });

      it('given the person moved the caret after the call was composed, when a selection target runs, then it uses the live caret', async (ctx) => {
        needs(ctx, 'userSelects');
        await start();
        await hooks().userSelects?.(3);
        await snapshot();
        unwrap(await client.insertAt('selection', 'X'));
        expect((await snapshot()).text).toBe('012X3456789');
      });

      it('given step text with CRLF, when applied, then changes and landed are in LF units', async () => {
        await start();
        const reply = unwrap(await client.replaceRange({ from: 0, to: 0 }, 'a\r\nb'));
        expect(reply).toMatchObject({ changes: [{ insert: 'a\nb' }], landed: [{ from: 0, to: 3 }] });
        expect((await snapshot()).text).toBe(`a\nb${TEN}`);
      });
    });

    describe('type', () => {
      const typeStep = (text: string): Step => ({ op: 'type', text });
      const advertises = async () => unwrap(await client.hello()).capabilities.steps.includes('type');

      it('given a caret, when a keystroke is typed, then it goes in there, the caret follows, the change says api, and one undo takes it back', async (ctx) => {
        // Given an engine that offers the step and a caret in the middle
        if (!(await advertises())) return ctx.skip();
        await start(TEN, { anchor: 3, head: 3 });
        const seen = record('changed');

        // When one keystroke is typed
        const result = unwrap(await client.transact([typeStep('x')]));

        // Then the text, the caret, the event and the undo step are those of one call
        expect(result.changes).toEqual([{ from: 3, to: 3, insert: 'x' }]);
        expect(result.landed).toEqual([null]);
        expect((await snapshot()).text).toBe('012x3456789');
        expect(result.selection.ranges[0]).toEqual({ anchor: 4, head: 4 });
        expect(seen).toMatchObject([{ origin: 'api', changes: [{ from: 3, to: 3, insert: 'x' }] }]);
        unwrap(await client.undo());
        expect((await snapshot()).text).toBe(TEN);
      });

      it('given a selection, when a keystroke is typed, then it replaces the selection', async (ctx) => {
        if (!(await advertises())) return ctx.skip();
        await start(TEN, { anchor: 2, head: 5 });

        unwrap(await client.transact([typeStep('Z')]));

        const after = await snapshot();
        expect(after.text).toBe('01Z56789');
        expect(after.selection.ranges[0]).toEqual({ anchor: 3, head: 3 });
      });

      it('given an empty text, a long text or another step beside it, when typed, then the call is invalid and nothing changes', async (ctx) => {
        if (!(await advertises())) return ctx.skip();
        const base = await start();

        expect(failed(await client.transact([typeStep('')])).code).toBe('invalid');
        expect(failed(await client.transact([typeStep('x'.repeat(65))])).code).toBe('invalid');
        expect(failed(await client.transact([typeStep('x'), replace(0, 0, 'y')])).code).toBe('invalid');
        expect(await snapshot()).toMatchObject({ text: TEN, revision: base });
      });

      it('given a call made at an older revision, when a keystroke is typed, then it is stale even with map, and at the head it applies', async (ctx) => {
        if (!(await advertises())) return ctx.skip();
        const base = await start();
        unwrap(await client.insertAt(0, 'A'));

        expect(failed(await client.transact([typeStep('x')], { at: base })).code).toBe('stale');
        expect(failed(await client.transact([typeStep('x')], { at: base, stale: 'map' })).code).toBe('stale');
        expect((await client.transact([typeStep('x')], { at: base + 1 })).ok).toBe(true);
      });
    });

    describe('selection', () => {
      it('given select(a, b) and select(b, a), when applied, then the main range keeps its direction', async () => {
        await start();
        expect(unwrap(await client.select(2, 6)).selection).toEqual({ ranges: [{ anchor: 2, head: 6 }], main: 0 });
        expect(unwrap(await client.select(6, 2)).selection).toEqual({ ranges: [{ anchor: 6, head: 2 }], main: 0 });
      });

      it('given an insert at p and a bare caret at p, when applied, then the caret lands after the text; a bare selection keeps inserted edge text outside', async () => {
        await start();
        const caret = await client.transact([replace(4, 4, 'ab'), { op: 'select', anchor: 4 }]);
        expect(unwrap(caret).selection.ranges[0]).toEqual({ anchor: 6, head: 6 });
        await start();
        const range = await client.transact([replace(2, 2, 'AA'), replace(4, 4, 'BB'), { op: 'select', anchor: 2, head: 4 }]);
        expect(unwrap(range).selection.ranges[0]).toEqual({ anchor: 4, head: 6 });
        expect((await snapshot()).text).toBe('01AA23BB456789');
      });

      it('given {at, assoc} points, when applied, then the caret lands before or after the text inserted at the point', async () => {
        await start();
        const before = await client.transact([replace(4, 4, 'XY'), { op: 'select', anchor: { at: 4, assoc: -1 } }]);
        expect(unwrap(before).selection.ranges[0]).toEqual({ anchor: 4, head: 4 });
        await start();
        const after = await client.transact([replace(4, 4, 'XY'), { op: 'select', anchor: { at: 4, assoc: 1 } }]);
        expect(unwrap(after).selection.ranges[0]).toEqual({ anchor: 6, head: 6 });
      });

      it('given {step} points, when applied, then they address the inserted text; points outside it, or on a select or missing step, are refused', async () => {
        await start();
        const end = await client.transact([replace(3, 3, 'XY'), { op: 'select', anchor: { step: 0, edge: 'end' } }]);
        expect(unwrap(end).selection.ranges[0]).toEqual({ anchor: 5, head: 5 });
        await start();
        const inner = await client.transact([replace(3, 3, 'XY'), { op: 'select', anchor: { step: 0, offset: 1 } }]);
        expect(unwrap(inner).selection.ranges[0]).toEqual({ anchor: 4, head: 4 });
        await start();
        const outside = await client.transact([replace(3, 3, 'XY'), { op: 'select', anchor: { step: 0, offset: 5 } }]);
        expect(failed(outside)).toMatchObject({ code: 'out-of-range', step: 0 });
        const onSelect = await client.transact([{ op: 'select', anchor: 0 }, { op: 'select', anchor: { step: 0 } }]);
        const missing = await client.transact([{ op: 'select', anchor: { step: 7 } }]);
        expect(failed(onSelect).code).toBe('invalid');
        expect(failed(missing).code).toBe('invalid');
      });

      it('given an edit with no select step, when applied, then the selection maps the way CodeMirror maps it, except that a selection inside replaced text collapses after the replacement', async () => {
        await start(TEN, { anchor: 4, head: 4 });
        expect(unwrap(await client.insertAt(4, 'XX')).selection.ranges[0]).toEqual({ anchor: 4, head: 4 });
        await start(TEN, { anchor: 2, head: 6 });
        const inward = await client.transact([replace(2, 2, 'A'), replace(6, 6, 'B')]);
        expect(unwrap(inward).selection.ranges[0]).toEqual({ anchor: 3, head: 7 });
        await start(TEN, { anchor: 3, head: 5 });
        const inverted = await client.replaceRange({ from: 2, to: 7 }, 'Z');
        expect(unwrap(inverted).selection.ranges[0]).toEqual({ anchor: 3, head: 3 });
      });

      it('given a select point past the end or two select steps, when applied, then they are out-of-range and invalid', async () => {
        await start();
        expect(failed(await client.select(99)).code).toBe('out-of-range');
        const two = await client.transact([{ op: 'select', anchor: 1 }, { op: 'select', anchor: 2 }]);
        expect(failed(two).code).toBe('invalid');
      });
    });

    describe('history', () => {
      it('given one apply and one multi-site apply, when undo runs once each, then the text is the one before the call', async () => {
        await start();
        unwrap(await client.replaceRange({ from: 2, to: 4 }, 'x'));
        expect(unwrap(await client.undo()).applied).toBe(true);
        expect((await snapshot()).text).toBe(TEN);
        unwrap(await client.transact([replace(0, 1, 'a'), replace(5, 6, 'b'), replace(9, 9, 'c')]));
        expect(unwrap(await client.undo()).applied).toBe(true);
        expect((await snapshot()).text).toBe(TEN);
      });

      it('given two quick calls, when undo runs, then each call is its own step', async () => {
        await start();
        unwrap(await client.insertAt(0, 'A'));
        unwrap(await client.insertAt(1, 'B'));
        unwrap(await client.undo());
        expect((await snapshot()).text).toBe(`A${TEN}`);
        unwrap(await client.undo());
        expect((await snapshot()).text).toBe(TEN);
      });

      it('given an apply and then a select-only call, when undo runs, then the edit is undone and the select made no step', async () => {
        await start();
        unwrap(await client.replaceRange({ from: 0, to: 1 }, 'X'));
        unwrap(await client.select(5));
        expect(unwrap(await client.undo()).applied).toBe(true);
        expect((await snapshot()).text).toBe(TEN);
        expect(unwrap(await client.undo()).applied).toBe(false);
      });

      it('given an empty stack, when undo runs, then nothing is applied, the revision is kept and no event fires', async () => {
        const base = await start();
        const seen = record('changed');
        const reply = await client.undo();
        expect(unwrap(reply)).toMatchObject({ applied: false, changes: [] });
        expect(reply.revision).toBe(base);
        expect(seen).toEqual([]);
      });

      it('given an undo and then a redo, when they run, then the text returns; after a new edit redo has nothing', async () => {
        await start();
        unwrap(await client.insertAt(0, 'Q'));
        unwrap(await client.undo());
        expect(unwrap(await client.redo()).applied).toBe(true);
        expect((await snapshot()).text).toBe(`Q${TEN}`);
        unwrap(await client.undo());
        unwrap(await client.insertAt(5, 'W'));
        expect(unwrap(await client.redo()).applied).toBe(false);
      });

      it('given an edit that moved the selection, when undo runs, then the selection from before the call returns', async () => {
        await start(TEN, { anchor: 2, head: 4 });
        unwrap(await client.transact([replace(6, 7, 'Q'), { op: 'select', anchor: 0 }]));
        expect((await snapshot()).selection.ranges[0]).toEqual({ anchor: 0, head: 0 });
        const undone = unwrap(await client.undo());
        expect(undone.selection.ranges[0]).toEqual({ anchor: 2, head: 4 });
      });

      it('given an undo and a redo, when they run, then each emits changed with origin history and one revision, and the mirror equals read', async () => {
        await start();
        unwrap(await client.insertAt(0, 'Q'));
        const mirror = await mirrorOf();
        const seen = record('changed');
        const before = (await snapshot()).revision;
        unwrap(await client.undo());
        unwrap(await client.redo());
        expect(seen).toMatchObject([{ origin: 'history', from: before, to: before + 1 }, { origin: 'history', from: before + 1, to: before + 2 }]);
        expect(mirror.text()).toBe((await snapshot()).text);
      });

      it('given typing, an api call and typing again, when undo runs, then there are three steps in that order', async (ctx) => {
        needs(ctx, 'userTypes');
        await start();
        await hooks().userTypes?.('a');
        unwrap(await client.insertAt(1 + TEN.length, 'Z'));
        await hooks().userTypes?.('b');
        expect((await snapshot()).text).toBe(`ab${TEN}Z`);
        unwrap(await client.undo());
        expect((await snapshot()).text).toBe(`a${TEN}Z`);
        unwrap(await client.undo());
        expect((await snapshot()).text).toBe(`a${TEN}`);
        unwrap(await client.undo());
        expect((await snapshot()).text).toBe(TEN);
        expect(unwrap(await client.undo()).applied).toBe(false);
      });
    });

    describe('revisions and rebasing', () => {
      it('given at equal to the head, when applied, then it applies with no mapping', async () => {
        const base = await start();
        expect((await client.replaceRange({ from: 0, to: 1 }, 'x', { at: base })).ok).toBe(true);
      });

      it('given an older at and the default policy, when applied, then it is stale with the head and the floor, and nothing applies', async () => {
        const base = await start();
        unwrap(await client.insertAt(0, 'A'));
        const reply = await client.replaceRange({ from: 0, to: 1 }, 'x', { at: base });
        expect(failed(reply)).toMatchObject({ code: 'stale', head: base + 1 });
        expect((await snapshot()).text).toBe(`A${TEN}`);
      });

      it('given at past the head, when applied, then it is invalid', async () => {
        const base = await start();
        expect(failed(await client.replaceRange({ from: 0, to: 1 }, 'x', { at: base + 1 })).code).toBe('invalid');
      });

      it('given an edit elsewhere and an older at with map, when applied, then a range before it is unchanged and one after it shifts', async () => {
        const base = await start();
        unwrap(await client.insertAt(8, 'XX'));
        unwrap(await client.transact([replace(1, 3, 'a'), replace(8, 9, 'b')], { at: base, stale: 'map' }));
        expect((await snapshot()).text).toBe('0a34567XXb9');
      });

      it('given the person typed before a range, when applied at the old revision with map, then the range follows the characters', async (ctx) => {
        needs(ctx, 'userTypes');
        const base = await start();
        await hooks().userTypes?.('ab');
        await snapshot();
        unwrap(await client.transact([replace(3, 5, 'X')], { at: base, stale: 'map' }));
        expect((await snapshot()).text).toBe('ab012X56789');
      });

      it('given inserts exactly at the from and to of a range after at, when applied with map, then the replace lands and the inserted text stays outside', async () => {
        const base = await start();
        unwrap(await client.transact([replace(3, 3, 'AA'), replace(6, 6, 'BB')]));
        unwrap(await client.transact([replace(3, 6, 'Z')], { at: base, stale: 'map' }));
        expect((await snapshot()).text).toBe('012AAZBB6789');
      });

      it('given the range text was deleted after at, when applied with map, then it is range-lost and nothing applies', async () => {
        const base = await start();
        unwrap(await client.replaceRange({ from: 3, to: 6 }, ''));
        const reply = await client.replaceRange({ from: 3, to: 6 }, 'x', { at: base, stale: 'map' });
        expect(failed(reply)).toMatchObject({ code: 'range-lost', step: 0 });
        expect((await snapshot()).text).toBe('0126789');
      });

      it('given the range text was replaced after at, when applied with map, then expect catches it and without expect the replacement is replaced', async () => {
        const base = await start();
        unwrap(await client.replaceRange({ from: 3, to: 6 }, 'QQ'));
        const guarded = await client.transact([replace(3, 6, 'x', '345')], { at: base, stale: 'map' });
        expect(failed(guarded)).toMatchObject({ code: 'text-mismatch', expected: '345', actual: 'QQ' });
        unwrap(await client.transact([replace(3, 6, 'x')], { at: base, stale: 'map' }));
        expect((await snapshot()).text).toBe('012x6789');
      });

      it('given more edits after at than the log keeps, when applied with map, then it is stale', async () => {
        const base = await start();
        const depth = unwrap(await client.hello()).capabilities.logDepth;
        for (let i = 0; i <= depth; i += 1) unwrap(await client.insertAt(0, 'x'));
        const reply = await client.replaceRange({ from: 0, to: 0 }, 'y', { at: base, stale: 'map' });
        expect(failed(reply).code).toBe('stale');
      });

      it('given the reply of an applied call was lost and the client retried with the same at, when it retries, then the retry is stale and the edit shows once', async (ctx) => {
        needs(ctx, 'dropNextReply');
        const base = await start();
        hooks().dropNextReply?.();
        const lost = await client.replaceRange({ from: 0, to: 0 }, 'R', { at: base });
        expect(failed(lost).code).toBe('transport');
        const retry = await client.replaceRange({ from: 0, to: 0 }, 'R', { at: base });
        expect(failed(retry).code).toBe('stale');
        expect((await snapshot()).text).toBe(`R${TEN}`);
      });

      it('given a call that is both mismatched and stale, or stale and out of range, when it runs, then the earlier check names it', async () => {
        const base = await start();
        unwrap(await client.insertAt(0, 'A'));
        expect(failed(await client.replaceRange({ from: 0, to: 1 }, 'x', { at: base, instance: 'other' })).code).toBe('instance-mismatch');
        expect(failed(await client.replaceRange({ from: 0, to: 99 }, 'x', { at: base })).code).toBe('stale');
      });
    });

    describe('events', () => {
      it('given an apply, when it resolves, then its changed event came first and the mirror already holds the change', async () => {
        await start();
        const mirror = await mirrorOf();
        const reply = await client.replaceRange({ from: 0, to: 2 }, 'xyz');
        expect(mirror.text()).toBe((await snapshot()).text);
        expect(mirror.revision()).toBe(reply.revision);
      });

      it('given api, user, history and load changes in sequence, when they run, then every changed has to = from + 1 and they chain', async (ctx) => {
        await start();
        const seen = record('changed');
        unwrap(await client.insertAt(0, 'a'));
        if (hooks().userTypes) await hooks().userTypes?.('b');
        unwrap(await client.undo());
        unwrap(await client.load('z'));
        await snapshot();
        const events = seen.filter((event) => event.type === 'changed');
        expect(events.length).toBeGreaterThanOrEqual(3);
        for (const [i, event] of events.entries()) {
          if (event.type !== 'changed') continue;
          expect(event.to).toBe(event.from + 1);
          const previous = events[i - 1];
          if (previous?.type === 'changed') expect(event.from).toBe(previous.to);
        }
        void ctx;
      });

      it('given a selection-only call and then an edit, when they run, then the first emits selection only and the second changed only', async () => {
        await start();
        const seen = record('changed', 'selection');
        unwrap(await client.select(4));
        expect(seen.map((event) => event.type)).toEqual(['selection']);
        unwrap(await client.insertAt(0, 'x'));
        expect(seen.map((event) => event.type)).toEqual(['selection', 'changed']);
      });

      it('given a listener that unsubscribes and one that throws, when edits happen, then the first hears nothing more and the engine and the others carry on', async () => {
        await start();
        const heard: string[] = [];
        const off = subject.engine.listen(['changed'], () => heard.push('x'));
        cleanups.push(subject.engine.listen(['changed'], () => { throw new Error('a bad subscriber'); }));
        const survivor = record('changed');
        unwrap(await client.insertAt(0, 'a'));
        off();
        unwrap(await client.insertAt(0, 'b'));
        expect(heard).toEqual(['x']);
        expect(survivor).toHaveLength(2);
        expect((await snapshot()).text).toBe(`ba${TEN}`);
      });

      it('given the person types, when the event arrives, then its origin is user and it has no label', async (ctx) => {
        needs(ctx, 'userTypes');
        await start();
        const seen = record('changed');
        await client.hello(); // a request after the subscription proves the page has it before the person types
        await hooks().userTypes?.('q');
        await snapshot();
        expect(seen).toHaveLength(1);
        expect(seen[0]).toMatchObject({ type: 'changed', origin: 'user' });
        expect(seen[0]).not.toHaveProperty('label');
      });

      it('given a listener for changed only, when an edit and a select-only call run, then only changed crosses the channel', async (ctx) => {
        needs(ctx, 'transportEvents');
        await start();
        record('changed');
        const mark = hooks().transportEvents?.().length ?? 0;
        unwrap(await client.insertAt(0, 'a'));
        unwrap(await client.select(2));
        await snapshot();
        expect(hooks().transportEvents?.().slice(mark)).toEqual(['changed']);
      });

      it('given a dropped event, when the next one arrives, then the mirror sees the gap, resyncs and equals read', async (ctx) => {
        needs(ctx, 'swallowNextEvent');
        await start();
        const mirror = await mirrorOf();
        hooks().swallowNextEvent?.();
        unwrap(await client.insertAt(0, 'a'));
        unwrap(await client.insertAt(0, 'b'));
        await new Promise<void>((resolve, reject) => {
          const deadline = Date.now() + 2000;
          const check = () => (mirror.text() === `ba${TEN}` ? resolve() : Date.now() > deadline ? reject(new Error(`the mirror stayed at ${mirror.text()}`)) : setTimeout(check, 10));
          check();
        });
        expect(mirror.revision()).toBe((await snapshot()).revision);
      });
    });

    describe('mirror', () => {
      it('given 150 seeded mixed operations, when each runs, then the mirror equals a fresh read after every step', async () => {
        await start();
        const mirror = await mirrorOf();
        let seed = 20260101;
        const next = (limit: number) => {
          seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
          return seed % limit;
        };
        for (let i = 0; i < 150; i += 1) {
          const { length, revision } = await snapshot();
          const a = next(length + 1);
          const b = Math.min(length, a + next(4));
          switch (next(7)) {
            case 0: await client.transact([replace(a, b, 'ab'.repeat(next(3)))], { at: mirror.revision() }); break;
            case 1: await client.transact([replace(a, a, 'm'), replace(b, b, 'n')], { at: revision - 1 < 0 ? 0 : revision - 1, stale: 'map' }); break;
            case 2: await client.moveRange({ from: a, to: b }, next(length + 1)); break;
            case 3: await client.undo(); break;
            case 4: await client.redo(); break;
            case 5: if (hooks().userTypes) await hooks().userTypes?.('t'); break;
            default: if (hooks().userSelects) await hooks().userSelects?.(a, b); else await client.select(a, b);
          }
          const fresh = await snapshot();
          expect({ text: mirror.text(), revision: mirror.revision(), selection: mirror.selection() }).toEqual({ text: fresh.text, revision: fresh.revision, selection: fresh.selection });
        }
      });

      it('given a tracked range, when edits come, then it follows them, flags touch and deletion, and stops when released', async () => {
        // Given a tracked range and a second one
        await start();
        const mirror = await mirrorOf();
        const tracked = mirror.track({ from: 4, to: 6 });
        const covered = mirror.track({ from: 3, to: 5 });

        // When edits come before, after, inside and over it
        unwrap(await client.insertAt(0, 'AA'));
        expect(tracked.get()).toEqual({ from: 6, to: 8 });
        unwrap(await client.insertAt(11, 'z'));
        expect(tracked.touched()).toBe(false);
        unwrap(await client.insertAt(7, 'x'));
        unwrap(await client.replaceRange({ from: 4, to: 8 }, ''));

        // Then it follows, is flagged, and a released range stops following
        expect(tracked.touched()).toBe(true);
        expect(covered.deleted()).toBe(true);
        const kept = tracked.get();
        tracked.release();
        unwrap(await client.insertAt(0, 'B'));
        expect(tracked.get()).toEqual(kept);
      });

      it('given a held range, when an api insert lands before it, then it shifts and reading it returns the original text', async () => {
        // Given a held range over "45"
        await start();
        const mirror = await mirrorOf();
        const held = mirror.track({ from: 4, to: 6 });

        // When text is inserted before it
        unwrap(await client.insertAt(1, 'XYZ'));

        // Then it holds the same characters at the new place
        const range = held.get();
        expect(range).toEqual({ from: 7, to: 9 });
        expect(unwrap(await client.getText(range as Range)).text).toBe('45');
        expect(held.touched()).toBe(false);
      });

      it('given a held range, when the person types before, at the edges and inside it, then the edges stay outside and an inside edit touches it', async (ctx) => {
        needs(ctx, 'userTypes', 'userSelects');
        // Given a held range over "345" and the caret before it
        await start(TEN, { anchor: 0, head: 0 });
        const mirror = await mirrorOf();
        const held = mirror.track({ from: 3, to: 6 });

        // When the person types before the range, at its start, at its end, then inside
        await hooks().userTypes?.('ab');
        await snapshot();
        expect(held.get()).toEqual({ from: 5, to: 8 });
        await hooks().userSelects?.(5);
        await snapshot();
        await hooks().userTypes?.('c');
        await snapshot();
        expect(held.get()).toEqual({ from: 6, to: 9 });
        await hooks().userSelects?.(9);
        await snapshot();
        await hooks().userTypes?.('d');
        await snapshot();
        expect(held.get()).toEqual({ from: 6, to: 9 });
        expect(held.touched()).toBe(false);
        await hooks().userSelects?.(7);
        await snapshot();
        await hooks().userTypes?.('e');
        await snapshot();

        // Then the range grew by the inner edit and still reads as the original characters plus it
        expect(held.get()).toEqual({ from: 6, to: 10 });
        expect(held.touched()).toBe(true);
        expect(unwrap(await client.getText(held.get() as Range)).text).toBe('3e45');
      });

      it('given a held range after an api edit, when it is undone and redone, then the range follows each step', async () => {
        // Given a held range and an api insert before it
        await start();
        const mirror = await mirrorOf();
        const held = mirror.track({ from: 4, to: 6 });
        unwrap(await client.insertAt(0, 'AB'));
        expect(held.get()).toEqual({ from: 6, to: 8 });

        // When the edit is undone, then redone
        unwrap(await client.undo());
        expect(held.get()).toEqual({ from: 4, to: 6 });
        unwrap(await client.redo());

        // Then the range is back after the insert and reads the same characters
        expect(held.get()).toEqual({ from: 6, to: 8 });
        expect(unwrap(await client.getText(held.get() as Range)).text).toBe('45');
      });

      it('given a selection and an api edit that moves it, when undo runs once, then text and selection are exactly as before the call', async () => {
        // Given a selection over "234"
        await start(TEN, { anchor: 2, head: 5 });
        const before = await snapshot();

        // When an edit that also moves the selection is applied and undone once
        unwrap(await client.transact([replace(7, 9, 'QQQ'), { op: 'select', anchor: 0 }]));
        expect((await snapshot()).text).toBe('0123456QQQ9');
        unwrap(await client.undo());

        // Then the text and the selection are those of before
        const after = await snapshot();
        expect(after.text).toBe(before.text);
        expect(after.selection).toEqual(before.selection);
      });

      it('given a new editor instance, when a tracked range is read, then it is null', async (ctx) => {
        needs(ctx, 'restart');
        await start();
        const mirror = await mirrorOf();
        const tracked = mirror.track({ from: 1, to: 2 });
        await hooks().restart?.();
        await new Promise<void>((resolve, reject) => {
          const deadline = Date.now() + 2000;
          const check = () => (tracked.get() === null ? resolve() : Date.now() > deadline ? reject(new Error('the tracked range survived a new instance')) : setTimeout(check, 10));
          check();
        });
        expect(tracked.get()).toBeNull();
      });
    });

    describe('config and view', () => {
      it('given a config patch, when it is applied, then the effective config comes back with the palette merged and the revision unchanged', async () => {
        const base = await start();
        const reply = await client.setConfig({ wrap: true, palette: { dark: true } });
        expect(unwrap(reply)).toMatchObject({ wrap: true, palette: { dark: true } });
        expect(typeof unwrap(reply).palette.cursor).toBe('string');
        expect(reply.revision).toBe(base);
      });

      it('given bad values, when configure runs, then it is invalid and names the path', async () => {
        await start();
        expect(failed(await client.setConfig({ tabSize: 0 }))).toMatchObject({ code: 'invalid', path: 'tabSize' });
        expect(failed(await client.setConfig({ palette: { cursor: 5 as unknown as string } })).code).toBe('invalid');
      });

      it('given a language outside the API\'s vocabulary, when configure runs, then it is unsupported, names the language and changes nothing', async () => {
        const base = await start();
        const reply = await client.setConfig({ language: 'cobol' as unknown as Language });
        expect(failed(reply)).toMatchObject({ code: 'unsupported', what: 'language' });
        expect(await snapshot()).toMatchObject({ text: TEN, revision: base });
      });

      it('given every language the engine offers, when configure({language}) is sent for each, then the config names it and the text, revision, selection and undo stack do not move and nothing is emitted', async () => {
        // Given a document, a selection and the language the page is in now (restored at the end: a page may be shared by the cases)
        const base = await start(TEN, { anchor: 2, head: 5 });
        const original = unwrap(await client.setConfig({})).language;
        const before = await snapshot();
        const seen = record('changed', 'selection');
        try {
          for (const language of unwrap(await client.hello()).capabilities.languages) {
            // When the language is configured
            const config = unwrap(await client.setConfig({ language }));

            // Then the effective config names it and the document is as it was
            expect(config.language).toBe(language);
            expect(await snapshot()).toEqual(before);
          }
          expect(unwrap(await client.undo()).applied).toBe(false);
          expect(seen).toEqual([]);
          expect((await snapshot()).revision).toBe(base);
        } finally {
          unwrap(await client.setConfig({ language: original }));
        }
      });

      it('given the same language configured twice, when each reply is read, then both are ok and identical', async () => {
        await start();
        const original = unwrap(await client.setConfig({})).language;
        try {
          const language = unwrap(await client.hello()).capabilities.languages[0] as Language;
          const first = await client.setConfig({ language });
          const second = await client.setConfig({ language });
          expect(first.ok && second.ok).toBe(true);
          expect(second).toEqual(first);
        } finally {
          unwrap(await client.setConfig({ language: original }));
        }
      });

      it('given a language configured and then a document loaded, when configure({}) is read, then the language is still the configured one', async () => {
        await start();
        const original = unwrap(await client.setConfig({})).language;
        try {
          const offered = unwrap(await client.hello()).capabilities.languages;
          const language = (offered.find((candidate) => candidate !== original) ?? original) as Language;
          unwrap(await client.setConfig({ language }));
          await start('abc');
          expect(unwrap(await client.setConfig({})).language).toBe(language);
        } finally {
          unwrap(await client.setConfig({ language: original }));
        }
      });

      it('given focus and blur, when they run, then focused flips and each change emits one focus event', async () => {
        await start();
        await client.blur();
        const seen = record('focus');
        expect(unwrap(await client.focus()).focused).toBe(true);
        expect(unwrap(await client.blur()).focused).toBe(false);
        expect(seen).toMatchObject([{ focused: true }, { focused: false }]);
      });
    });

    describe('ordering', () => {
      it('given 50 pipelined applies each at the revision the last would produce, when they run, then they apply in send order', async () => {
        const base = await start();
        const letters = Array.from({ length: 50 }, (_, i) => String.fromCharCode(97 + (i % 26)));
        const replies = await Promise.all(letters.map((letter, i) => client.insertAt(TEN.length + i, letter, { at: base + i })));
        expect(replies.every((reply) => reply.ok)).toBe(true);
        expect((await snapshot()).text).toBe(TEN + letters.join(''));
      });

      it('given an apply and an immediate read not awaited in between, when both resolve, then the read sees the post-apply text', async () => {
        await start();
        const [, read] = await Promise.all([client.replaceRange({ from: 0, to: 1 }, 'Z'), client.getText()]);
        expect(unwrap(read).text.startsWith('Z1')).toBe(true);
      });
    });

    describe('transport', () => {
      it('given a silent page, when a request is sent, then the engine rejects with a timeout and the client resolves a transport reply', async (ctx) => {
        needs(ctx, 'silentPage');
        const own = await make({ bound: true });
        extras.push(own);
        const bare = createEditorClient(own.engine);
        unwrap(await bare.load(TEN));
        const resume = own.hooks.silentPage?.();
        const rejected = await own.engine.request('read', {}).catch((error: unknown) => error);
        const reply = await bare.getText();
        resume?.();
        expect(rejected).toMatchObject({ code: 'timeout' });
        expect(failed(reply)).toMatchObject({ code: 'transport', cause: 'timeout' });
      });

      it('given a closed channel, when a request is sent, then it fails as closed', async (ctx) => {
        needs(ctx, 'closeTransport');
        const own = await make({ bound: true });
        extras.push(own);
        own.hooks.closeTransport?.();
        const reply = await createEditorClient(own.engine).getText();
        expect(failed(reply)).toMatchObject({ code: 'transport', cause: 'closed' });
      });

      it('given a 200 KB document, when it is read, then it round-trips', async (ctx) => {
        needs(ctx, 'transportEvents');
        const big = `${'x'.repeat(99)}\n`.repeat(2000);
        await start(big);
        expect((await snapshot()).text).toHaveLength(big.length);
      });

      it('given every reply and event, when they cross the channel, then they are JSON-safe', async (ctx) => {
        needs(ctx, 'transportEvents');
        await start();
        const seen = record('changed', 'selection', 'focus');
        const replies = [await client.hello(), await client.getText(), await client.insertAt(0, 'a'), await client.select(1), await client.undo(), await client.setConfig({ wrap: false }), await client.focus()];
        for (const value of [...replies, ...seen]) expect(JSON.parse(JSON.stringify(value))).toEqual(value);
      });

      it('given a pending call and a new instance, when ready arrives, then the call rejects as restarted', async (ctx) => {
        needs(ctx, 'silentPage', 'restart');
        const own = await make({ bound: true });
        extras.push(own);
        const bare = createEditorClient(own.engine);
        unwrap(await bare.hello());
        const resume = own.hooks.silentPage?.();
        const pending = own.engine.request('read', {}).catch((error: unknown) => error);
        await own.hooks.restart?.();
        resume?.();
        expect(await pending).toMatchObject({ code: 'restarted' });
      });
    });
  });
}
