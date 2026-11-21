import { WS_METHODS } from "@t3tools/contracts";
import { archiveThread, createAtoms, mutateProject, readHistory, readSession } from "@t3tools/client-runtime/state";

const atoms = createAtoms(runtime);

export default function App() {
  const write = useCommand(atoms.write);
  probe(WS_METHODS.reachedProbe);
  return <Button onPress={() => archiveThread({ threadId }).then(() => readHistory(input)).then(readSession).then(mutateProject)}>Don't wait</Button>;
}

export function create() {
  return dispatch({ type: "reached.create" });
}

export const send = (tag: string) => rpcClient[tag]({});
