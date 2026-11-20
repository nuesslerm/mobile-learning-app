import { pyodideSpike } from '../di/pyodideSpike';
import { PyodideSpikeScreen } from '../ui/PyodideSpikeScreen';

export default function PyodideSpikeRoute() {
  return <PyodideSpikeScreen assets={pyodideSpike.assets} pageHtml={pyodideSpike.pageHtml} />;
}
