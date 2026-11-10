import type { AgentationLogger } from '../../domain/types';
import { noopLogger } from '../../domain/types';
import type { DocumentNode, DocumentTreeDriver } from '../domain/types';

export function documentNodeToCaptureResult(node: DocumentNode): import('../../domain/types').CaptureResult {
  return {
    componentName: node.name,
    source: node.source,
    sourceFile: node.sourceFile,
    rect: node.bounds,
    props: node.props,
    ownerChain: node.ownerChain,
  };
}

export class DocumentNodeService {
  private snapshot: DocumentNode | null = null;
  private selectedId: string | null = null;
  private subscribers = new Set<() => void>();

  constructor(
    private driver: DocumentTreeDriver,
    private logger: AgentationLogger = noopLogger,
  ) {}

  async takeSnapshot(): Promise<DocumentNode | null> {
    try {
      this.snapshot = await this.driver.takeSnapshot();
      this.logger.withMetadata({ nodeCount: this.countNodes(this.snapshot) }).info('agentation.snapshot');
      this.emit();
      return this.snapshot;
    } catch (err) {
      this.logger.withMetadata({ err: String(err) }).info('agentation.snapshot_failed');
      this.snapshot = null;
      this.emit();
      return null;
    }
  }

  async refresh(): Promise<void> {
    await this.takeSnapshot();
  }

  get currentSnapshot(): DocumentNode | null {
    return this.snapshot;
  }

  get currentSelectedId(): string | null {
    return this.selectedId;
  }

  async queryAtPoint(x: number, y: number): Promise<DocumentNode | undefined> {
    // Web driver implements a native hit-test (`document.elementsFromPoint`)
    // that respects z-index / overlap / pointer-events, which bounding-box DFS
    // over the snapshot tree does not. Prefer it when the driver exposes it.
    if (typeof this.driver.queryAtPoint === 'function') {
      const result = await this.driver.queryAtPoint(x, y);
      if (result) return result;
      // Driver had no hit at this point — fall through to the tree walk in
      // case the snapshot was stale (e.g. driver built before navigation).
      if (!this.snapshot) {
        await this.takeSnapshot();
        if (!this.snapshot) return undefined;
      }
      return this.findDeepestAtPoint(this.snapshot, x, y);
    }

    if (!this.snapshot) {
      await this.takeSnapshot();
    }
    if (!this.snapshot) return undefined;
    return this.findDeepestAtPoint(this.snapshot, x, y);
  }

  queryById(id: string): DocumentNode | undefined {
    const snapshot = this.snapshot;
    if (!snapshot) return undefined;
    return this.findById(snapshot, id);
  }

  queryByName(name: string): DocumentNode[] {
    const snapshot = this.snapshot;
    if (!snapshot) return [];
    const out: DocumentNode[] = [];
    this.walk(snapshot, (n) => {
      if (n.name === name) out.push(n);
    });
    return out;
  }

  queryBySource(file: string): DocumentNode[] {
    const snapshot = this.snapshot;
    if (!snapshot) return [];
    const out: DocumentNode[] = [];
    this.walk(snapshot, (n) => {
      if (n.source?.fileName === file || n.sourceFile === file) out.push(n);
    });
    return out;
  }

  queryByTestId(testId: string): DocumentNode[] {
    const snapshot = this.snapshot;
    if (!snapshot) return [];
    const out: DocumentNode[] = [];
    this.walk(snapshot, (n) => {
      if (n.props.testID === testId) out.push(n);
    });
    return out;
  }

  query(predicate: (node: DocumentNode) => boolean): DocumentNode[] {
    const snapshot = this.snapshot;
    if (!snapshot) return [];
    const out: DocumentNode[] = [];
    this.walk(snapshot, (n) => {
      if (predicate(n)) out.push(n);
    });
    return out;
  }

  get selectedNode(): DocumentNode | undefined {
    if (!this.snapshot || !this.selectedId) return undefined;
    return this.findById(this.snapshot, this.selectedId);
  }

  selectNode(id: string): void {
    this.selectedId = id;
    this.emit();
  }

  subscribe(callback: () => void): () => void {
    this.subscribers.add(callback);
    return () => this.subscribers.delete(callback);
  }

  private emit(): void {
    this.subscribers.forEach((cb) => cb());
  }

  private countNodes(node: DocumentNode | null): number {
    if (!node) return 0;
    return 1 + node.children.reduce((sum, c) => sum + this.countNodes(c), 0);
  }

  private walk(node: DocumentNode, fn: (n: DocumentNode) => void): void {
    fn(node);
    node.children.forEach((c) => this.walk(c, fn));
  }

  private findById(node: DocumentNode, id: string): DocumentNode | undefined {
    if (node.id === id) return node;
    for (const child of node.children) {
      const found = this.findById(child, id);
      if (found) return found;
    }
    return undefined;
  }

  private findDeepestAtPoint(node: DocumentNode, x: number, y: number): DocumentNode | undefined {
    const contains =
      x >= node.bounds.x &&
      x <= node.bounds.x + node.bounds.width &&
      y >= node.bounds.y &&
      y <= node.bounds.y + node.bounds.height;
    if (!contains) return undefined;

    for (const child of node.children) {
      const found = this.findDeepestAtPoint(child, x, y);
      if (found) return found;
    }
    return node;
  }
}
