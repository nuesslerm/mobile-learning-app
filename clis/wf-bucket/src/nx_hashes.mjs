// Prints the Nx task hashes of a workspace on one line of JSON, for `wf test drift`:
//   {"schema":1,"runtime":{<command>:<its output>},"tasks":{"<project>:<target>":{"hash":"..","inputs":{<input>:<hash>}}}}
// Run as `node --input-type=module - <workspace> <targets,> [<projects,>]` with this text on stdin. It hashes the way `nx run-many`
// does (the same graph, plugins and runtime inputs) and runs nothing. The deep imports are Nx's own task hasher: the flow test
// of `wf test drift` runs it against the installed Nx, so an Nx upgrade that moves them fails there.
import { createRequire } from 'node:module';
import { execSync } from 'node:child_process';

const [root, targets, projects] = process.argv.slice(2);
process.chdir(root);
process.env.NX_DAEMON = 'false';
process.env.NX_NO_CLOUD = 'true';
const nx = createRequire(`${root}/package.json`);
const { readNxJson } = nx('nx/src/config/nx-json');
const { createProjectGraphAsync } = nx('nx/src/project-graph/project-graph');
const { createTaskGraph } = nx('nx/src/tasks-runner/create-task-graph');
const { createTaskHasher } = nx('nx/src/hasher/create-task-hasher');
const { hashTasks } = nx('nx/src/hasher/hash-task');

const nxJson = readNxJson(root);
const graph = await createProjectGraphAsync({ exitOnError: true });
const names = projects ? projects.split(',') : Object.keys(graph.nodes);
const taskGraph = createTaskGraph(graph, nxJson.targetDependencies ?? {}, names, targets.split(','), undefined, {}, true);
const tasks = Object.values(taskGraph.tasks);
await hashTasks(createTaskHasher(graph, nxJson, {}), graph, taskGraph, Object.fromEntries(tasks.map((t) => [t.id, process.env])), null);

const table = {};
const commands = new Set();
for (const task of tasks.sort((a, b) => a.id.localeCompare(b.id))) {
  const { command, nodes, implicitDeps, runtime } = task.hashDetails;
  const inputs = { command, ...nodes, ...implicitDeps, ...runtime };
  for (const input of Object.keys(inputs)) if (input.startsWith('runtime:')) commands.add(input.slice('runtime:'.length));
  table[task.id] = { hash: task.hash, inputs };
}

// the value behind each runtime input's hash, so a report can say node 24.13.1 against 22.22.0 rather than two numbers
const values = {};
for (const command of [...commands].sort()) {
  try {
    values[command] = execSync(command, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch (e) {
    values[command] = `failed: ${e.status ?? e.message}`;
  }
}
console.log(JSON.stringify({ schema: 1, runtime: values, tasks: table }));
