import { routerHarness, screen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { createCourseProgress, createFlashCardProgress, createMemoryTamilStorage, tamilTestCurriculum } from '@/features/tamil-writing/testing';

import { app } from './app';

async function setup() {
  const sections = await tamilTestCurriculum.listSections();
  const topic = sections[0].topics[0];
  const task = topic.tasks[0];
  const courseProgress = createCourseProgress(createMemoryTamilStorage());
  const played: boolean[] = [];
  const options = {
    overrides: {
      tamilWriting: {
        courseProgress, audio: { async play(_id: string, slow: boolean) { played.push(slow); }, stop() {} },
      },
    },
  };
  const route = `/tamil/lessons/${topic.id}/${task.id}`;
  return { topic, task, route, options, courseProgress, played };
}

describe('Offline Tamil lessons', () => {
  it('Given the full library, When a learner searches All Topics by an exercise, Then its task opens in the owning topic', async () => {
    const { options } = await setup();
    const sections = await tamilTestCurriculum.listSections();
    const topic = sections[sections.length - 1].topics.at(-1)!;
    const task = topic.tasks.at(-1)!;
    const exercise = task.exercises.at(-1)!;
    const library = await app.render('/tamil', options);
    await waitFor(() => screen.getByText('31 topics · 160 tasks · 1,014 exercises'));
    await library.tap('TamilAllTopics');
    expect(routerHarness().calls()).toContainEqual(['push', '/tamil/all-topics']);
    const list = await app.render('/tamil/all-topics', options);
    await waitFor(() => screen.getByTestId('TamilTaskSearch'));
    await list.type('TamilTaskSearch', exercise.english);
    await list.tap(`TamilTask-${task.id}`);
    expect(routerHarness().calls()).toContainEqual(['push', {
      pathname: '/tamil/lessons/[groupId]/[taskId]', params: { groupId: topic.id, taskId: task.id },
    }]);
  });

  it('Given the Tamil Library, When a section cover opens, Then only its lessons are listed', async () => {
    const { options } = await setup();
    const [first, second] = await tamilTestCurriculum.listSections();
    const library = await app.render('/tamil', options);
    await waitFor(() => screen.getByTestId(`TamilCollection-${second.id}`));
    await library.tap(`TamilCollection-${second.id}`);
    expect(routerHarness().calls()).toContainEqual(['push', { pathname: '/tamil/all-topics', params: { section: second.id } }]);
    await app.render('/tamil/all-topics', { ...options, search: { section: second.id } });
    await waitFor(() => screen.getByTestId(`TamilTask-${second.topics[0].tasks[0].id}`));
    expect(screen.queryByTestId(`TamilTask-${first.topics[0].tasks[0].id}`)).toBeNull();
  });

  it('Given a collection, When Handwriting is selected in the dock, Then its catalog opens', async () => {
    const { options } = await setup();
    const collection = await app.render('/tamil/all-topics', options);
    await collection.tap('TamilHandwritingEntry');
    expect(routerHarness().calls()).toContainEqual(['navigate', '/tamil/handwriting']);
  });

  it('Given the library, When a topic and Practice are opened, Then the first task is ready', async () => {
    const { options, route, topic, task } = await setup();
    const session = await app.render('/tamil', options);
    await waitFor(() => screen.getByTestId('TamilTopic-greetings'));
    await session.tap('TamilTopic-greetings');
    expect(routerHarness().calls()).toContainEqual(['push', {
      pathname: '/tamil/lessons/[groupId]', params: { groupId: topic.id },
    }]);
    const group = await app.render(`/tamil/lessons/${topic.id}`, options);
    await waitFor(() => screen.getByTestId('TamilPractice'));
    await group.tap('TamilPractice');
    expect(routerHarness().calls()).toContainEqual(['push', {
      pathname: '/tamil/lessons/[groupId]/[taskId]', params: { groupId: topic.id, taskId: task.id },
    }]);
    await app.render(route, options);
    await waitFor(() => screen.getByTestId('TamilStartPractice'));
    expect(screen.getByText('Worked example')).toBeTruthy();
  });

  it('Given a tile answer, When the learner switches to typing and checks it, Then it is retained and scored', async () => {
    const { route, options, task, courseProgress } = await setup();
    const session = await app.render(route, options);
    await waitFor(() => screen.getByTestId('TamilStartPractice'));
    await session.tap('TamilStartPractice');
    await session.tap('TamilTile-bank-0');
    await session.tap('TamilSwitchAnswer');
    expect((screen.getByTestId('TamilTypedAnswer') as HTMLTextAreaElement).value).toBe(task.exercises[0].words[0]);
    await session.tap('TamilSwitchAnswer');
    await session.tap('TamilTile-answer-0');
    await session.tap('TamilTile-bank-0');
    await session.tap('TamilCheck');
    await waitFor(() => screen.getByText('Correct'));
    expect((await courseProgress.read()).tasks[task.id]?.answers[task.exercises[0].id]).toBe('correct');
  });

  it('Given a resumed listening prompt, When played at both speeds and skipped, Then it is recorded for review', async () => {
    const { courseProgress, topic, task, route, options, played } = await setup();
    const targetIndex = task.exercises.findIndex((exercise) => exercise.kind === 'listen');
    for (const exercise of task.exercises.slice(0, targetIndex)) {
      await courseProgress.update({ kind: 'answer', topicId: topic.id, taskId: task.id, exerciseId: exercise.id, outcome: 'correct' });
    }
    const session = await app.render(route, options);
    await waitFor(() => screen.getByTestId('TamilPlayAudio'));
    await session.tap('TamilPlayAudio');
    await session.tap('TamilSlowAudio');
    await session.tap('TamilSkipListening');
    expect(played).toEqual([false, true]);
    await waitFor(async () => {
      expect((await courseProgress.read()).tasks[task.id]?.answers[task.exercises[targetIndex].id]).toBe('skipped');
    });
  });

  it('Given a fill-in prompt, When a tile is put in the blank and checked, Then the missing word is scored', async () => {
    const { courseProgress, options } = await setup();
    const subjects = (await tamilTestCurriculum.listSections()).flatMap((section) => section.topics);
    const fillTopic = subjects.find((item) => item.tasks.some((entry) => entry.exercises.some((exercise) => exercise.kind === 'fill')))!;
    const fillTask = fillTopic.tasks.find((entry) => entry.exercises.some((exercise) => exercise.kind === 'fill'))!;
    const index = fillTask.exercises.findIndex((exercise) => exercise.kind === 'fill');
    for (const exercise of fillTask.exercises.slice(0, index)) {
      await courseProgress.update({ kind: 'answer', topicId: fillTopic.id, taskId: fillTask.id, exerciseId: exercise.id, outcome: 'correct' });
    }
    const session = await app.render(`/tamil/lessons/${fillTopic.id}/${fillTask.id}`, options);
    if (index === 0) {
      await waitFor(() => screen.getByTestId('TamilStartPractice'));
      await session.tap('TamilStartPractice');
    }
    await waitFor(() => screen.getByTestId('TamilExercise-fill'));
    await session.tap('TamilTile-bank-0');
    await session.tap('TamilCheck');
    await waitFor(() => screen.getByText('Correct'));
  });
});

describe('Tamil flash cards', () => {
  it('Given a task, When flipping and rating in either direction, Then the schedule survives a return to practice', async () => {
    const { topic, task, route, options, courseProgress, played } = await setup();
    const storage = createMemoryTamilStorage();
    let fail = true;
    const flashCards = createFlashCardProgress({ ...storage, async setItem(key, value) {
      if (fail) { fail = false; throw new Error('Disk full'); }
      await storage.setItem(key, value);
    } });
    const props = { overrides: { tamilWriting: { ...options.overrides.tamilWriting, flashCards } } };
    const list = await app.render('/tamil/lessons/' + topic.id, props);
    await waitFor(() => screen.getByTestId('TamilFlashCards-' + task.id));
    await list.tap('TamilFlashCards-' + task.id);
    expect(routerHarness().calls()).toContainEqual(['push', {
      pathname: '/tamil/lessons/[groupId]/[taskId]', params: { groupId: topic.id, taskId: task.id, mode: 'flash-cards' },
    }]);
    const flash = { ...props, search: { mode: 'flash-cards' } };
    const session = await app.render(route, flash);
    await waitFor(() => screen.getByTestId('TamilFlashReveal'));
    expect(screen.queryByText(task.exercises[0].tamil)).toBeNull();
    expect(screen.queryByTestId('TamilFlashAudio')).toBeNull();
    await session.tap('TamilFlashReveal');
    await session.tap('TamilFlashAudio');
    expect(played).toEqual([false]);
    await session.tap('TamilFlashRate-Good');
    await waitFor(() => screen.getByText('Couldn’t finish that action. Try again.'));
    expect(screen.getByText(task.exercises[0].tamil)).toBeTruthy();
    await session.tap('TamilFlashRate-Good');
    await waitFor(() => screen.getByText(task.exercises[1].english));
    await session.tap('TamilFlash-tamil-first');
    await waitFor(() => screen.getByText(task.exercises[0].tamil));
    expect(screen.getByTestId('TamilFlashAudio')).toBeTruthy();
    expect(screen.queryByText(task.exercises[0].english)).toBeNull();
    for (const phrase of task.exercises) {
      await waitFor(() => screen.getByText(phrase.tamil));
      await session.tap('TamilFlashReveal');
      expect(screen.getByText(phrase.english)).toBeTruthy();
      await session.tap('TamilFlashRate-Easy');
    }
    await waitFor(() => screen.getByText(/Next review/));
    session.unmount();
    const reopened = await app.render(route, flash);
    await waitFor(() => screen.getByText(/Next review/));
    await reopened.tap('TamilFlash-english-first');
    await waitFor(() => screen.getByText(task.exercises[1].english));
    expect((await courseProgress.read()).tasks).toEqual({});
  });
});
