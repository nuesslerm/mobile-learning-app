/**
 * Headless hook for `rn-agentation`. Owns all session state (active, annotations,
 * mode, selection, panel). Host apps render their own FAB / overlay and dispatch
 * taps through the returned handlers.
 */
import * as Clipboard from 'expo-clipboard';
import * as React from 'react';
import { Platform, useWindowDimensions } from 'react-native';

import { DocumentNodeService, documentNodeToCaptureResult } from '../document/service/DocumentNodeService';
import { noopLogger, type Annotation } from '../domain/types';
import { pointToWireframe } from '../geometry/wireframeGeometry';
import { toMarkdown } from '../markdown/toMarkdown';
import { toWireframeMarkdown } from '../markdown/toWireframeMarkdown';
import { createSession, postAnnotation } from '../api/agentationApi';
import { useAgentationSessionStore } from '../state/agentationSessionStore';

import type {
  AgentationHandle,
  AgentationMode,
  AgentationOptions,
} from './types';

const PLATFORM: 'web' | 'native' = Platform.OS === 'web' ? 'web' : 'native';

/**
 * Internal state hook for `rn-agentation`. Created once inside
 * {@link AgentationProvider} so the handle is shared by every consumer.
 */
export function useAgentationState(
  options: AgentationOptions,
  documentService: DocumentNodeService,
): AgentationHandle {
  const {
    enabled = true,
    logger = noopLogger,
    endpoint,
  } = options;
  const { width, height } = useWindowDimensions();

  const [active, setActive] = React.useState(false);
  const [panelOpen, setPanelOpen] = React.useState(false);
  const [annotations, setAnnotations] = React.useState<Annotation[]>([]);
  const [mode, setModeState] = React.useState<AgentationMode>('annotate');
  const [selectedType, setSelectedType] = React.useState('Button');
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [placementPending, setPlacementPending] = React.useState(false);

  // Monotonic local id counter — kept in a ref so successive taps never collide
  // even when annotations are cleared in between.
  const idRef = React.useRef(0);
  const nextId = React.useCallback((): string => {
    idRef.current += 1;
    return String(idRef.current);
  }, []);

  const sessionId = useAgentationSessionStore((s) => s.sessionId);
  const setSessionId = useAgentationSessionStore((s) => s.setSessionId);

  const ensureSession = React.useCallback(async (): Promise<string | null> => {
    if (!endpoint) return null;
    if (sessionId) return sessionId;
    const page =
      typeof window !== 'undefined' ? window.location.pathname : undefined;
    const res = await createSession({ endpoint }, { page });
    setSessionId(res.id);
    return res.id;
  }, [endpoint, sessionId, setSessionId]);

  const toggleActive = React.useCallback(() => {
    setActive((prev) => {
      const next = !prev;
      if (next) {
        setPanelOpen(true);
      } else {
        setPlacementPending(false);
      }
      logger
        .withMetadata({ active: next, platform: PLATFORM })
        .info('agentation.toggle');
      return next;
    });
  }, [logger]);

  const setMode = React.useCallback((next: AgentationMode) => {
    setModeState(next);
    setPlacementPending(false);
  }, []);

  const startPlacement = React.useCallback(() => {
    setPlacementPending(true);
    logger.withMetadata({ platform: PLATFORM }).info('agentation.startPlacement');
  }, [logger]);

  const cancelPlacement = React.useCallback(() => {
    setPlacementPending(false);
    logger.withMetadata({ platform: PLATFORM }).info('agentation.cancelPlacement');
  }, [logger]);

  const annotateAt = React.useCallback(
    async (x: number, y: number): Promise<void> => {
      if (!enabled) return;

      await documentService.refresh();
      const node = await documentService.queryAtPoint(x, y);
      const result = node ? documentNodeToCaptureResult(node) : { componentName: 'unknown' };
      const id = nextId();
      const annotation: Annotation = {
        ...result,
        id,
        note: '',
        platform: PLATFORM,
        kind: 'annotate',
        selected: true,
        documentNodeId: node?.id,
        documentNodeProps: node?.props,
        documentNodeSubtree: node,
      };

      setAnnotations((prev) => [
        ...prev.map((a) => ({ ...a, selected: false })),
        annotation,
      ]);
      setSelectedId(id);
      setPanelOpen(true);

      if (endpoint) {
        try {
          const sid = await ensureSession();
          if (sid) {
            await postAnnotation({ endpoint }, sid, {
              kind: annotation.kind ?? 'annotate',
              componentName: annotation.componentName,
              sourceFile: annotation.source?.fileName ?? annotation.sourceFile,
              sourceLine: annotation.source?.lineNumber,
              note: annotation.note,
              rect: annotation.rect,
              wireframe: annotation.wireframe
                ? {
                    componentType: annotation.wireframe.componentType,
                    xRatio: annotation.wireframe.xRatio,
                    yRatio: annotation.wireframe.yRatio,
                    widthRatio: annotation.wireframe.widthRatio,
                    heightRatio: annotation.wireframe.heightRatio,
                  }
                : undefined,
              documentNodeId: annotation.documentNodeId,
              documentNodeProps: annotation.documentNodeProps,
              documentNodeSubtree: annotation.documentNodeSubtree,
            });
          }
        } catch (err) {
          logger.withMetadata({ err }).info('agentation.httpPostFailed');
        }
      }

      logger
        .withMetadata({
          component: result.componentName,
          file: result.source?.fileName ?? null,
          line: result.source?.lineNumber ?? null,
          platform: PLATFORM,
        })
        .info('agentation.annotate');
    },
    [enabled, documentService, logger, endpoint, ensureSession, nextId]
  );

  const placeWireframeAt = React.useCallback(
    (x: number, y: number): void => {
      if (!enabled) return;
      const id = nextId();
      const annotation: Annotation = {
        id,
        componentName: selectedType,
        note: '',
        platform: PLATFORM,
        kind: 'placement',
        wireframe: pointToWireframe(x, y, { width, height }, selectedType),
        selected: true,
      };
      setAnnotations((prev) => [
        ...prev.map((a) => ({ ...a, selected: false })),
        annotation,
      ]);
      setSelectedId(id);
      setPanelOpen(true);
      setPlacementPending(false);
      logger
        .withMetadata({ componentType: selectedType, x, y })
        .info('agentation.place');
    },
    [enabled, selectedType, width, height, logger, nextId]
  );

  const selectAnnotation = React.useCallback((id: string) => {
    setSelectedId(id);
    setAnnotations((prev) =>
      prev.map((a) =>
        a.id === id ? { ...a, selected: true } : { ...a, selected: false }
      )
    );
  }, []);

  const moveWireframe = React.useCallback(
    (id: string, dx: number, dy: number) => {
      setAnnotations((prev) =>
        prev.map((a) => {
          if (a.id !== id || !a.wireframe) return a;
          const nextX = Math.max(
            0,
            Math.min(a.wireframe.xRatio + dx / width, 1)
          );
          const nextY = Math.max(
            0,
            Math.min(a.wireframe.yRatio + dy / height, 1)
          );
          return {
            ...a,
            wireframe: { ...a.wireframe, xRatio: nextX, yRatio: nextY },
          };
        })
      );
    },
    [width, height]
  );

  const resizeWireframe = React.useCallback(
    (id: string, dw: number, dh: number) => {
      setAnnotations((prev) =>
        prev.map((a) => {
          if (a.id !== id || !a.wireframe) return a;
          const nextW = Math.max(0.02, a.wireframe.widthRatio + dw / width);
          const nextH = Math.max(0.02, a.wireframe.heightRatio + dh / height);
          return {
            ...a,
            wireframe: {
              ...a.wireframe,
              widthRatio: nextW,
              heightRatio: nextH,
            },
          };
        })
      );
    },
    [width, height]
  );

  const updateNote = React.useCallback((id: string, note: string) => {
    setAnnotations((prev) =>
      prev.map((a) => (a.id === id ? { ...a, note } : a))
    );
  }, []);

  const updateWireframeProps = React.useCallback(
    (id: string, props: Record<string, unknown>) => {
      setAnnotations((prev) =>
        prev.map((a) => {
          if (a.id !== id || !a.wireframe) return a;
          return {
            ...a,
            wireframe: {
              ...a.wireframe,
              props: { ...a.wireframe.props, ...props },
            },
          };
        })
      );
      logger
        .withMetadata({
          annotationId: id,
          propNames: Object.keys(props),
          platform: PLATFORM,
        })
        .info('agentation.updateWireframeProps');
    },
    [logger]
  );

  const deleteAnnotation = React.useCallback(
    (id: string) => {
      setAnnotations((prev) => prev.filter((a) => a.id !== id));
      setSelectedId((prev) => (prev === id ? null : prev));
      logger
        .withMetadata({ annotationId: id, platform: PLATFORM })
        .info('agentation.delete');
    },
    [logger]
  );

  const clear = React.useCallback(() => {
    setAnnotations([]);
    setSelectedId(null);
    logger.withMetadata({ platform: PLATFORM }).info('agentation.clear');
  }, [logger]);

  const copyToClipboard = React.useCallback(async () => {
    const hasWireframe = annotations.some((a) => a.kind === 'placement');
    const markdown = hasWireframe
      ? toWireframeMarkdown(annotations)
      : toMarkdown(annotations);
    await Clipboard.setStringAsync(markdown);
    logger
      .withMetadata({
        count: annotations.length,
        platform: PLATFORM,
        hasWireframe,
      })
      .info('agentation.copy');
  }, [annotations, logger]);

  return React.useMemo<AgentationHandle>(
    () => ({
      active,
      annotations,
      selectedId,
      mode,
      selectedType,
      panelOpen,
      placementPending,
      toggleActive,
      setMode,
      setSelectedType,
      setPanelOpen,
      startPlacement,
      cancelPlacement,
      annotateAt,
      placeWireframeAt,
      moveWireframe,
      resizeWireframe,
      selectAnnotation,
      updateNote,
      updateWireframeProps,
      deleteAnnotation,
      copyToClipboard,
      clear,
    }),
    [
      active,
      annotations,
      selectedId,
      mode,
      selectedType,
      panelOpen,
      placementPending,
      startPlacement,
      cancelPlacement,
      toggleActive,
      annotateAt,
      placeWireframeAt,
      moveWireframe,
      resizeWireframe,
      selectAnnotation,
      updateNote,
      updateWireframeProps,
      deleteAnnotation,
      copyToClipboard,
      clear,
    ]
  );
}
