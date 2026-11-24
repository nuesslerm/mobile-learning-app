import { routerHarness, screen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { createCourseProgress, createMemoryTamilStorage, tamilTestCurriculum } from '@/features/tamil-writing/testing';

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
