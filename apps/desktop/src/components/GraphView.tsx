/**
 * GraphView — react-flow visualisation of the session DAG.
 *
 * Responsibilities:
 *  - Fetch the full graph via the `get_graph` bridge call on mount and
 *    whenever the parent signals a refresh (new node created, project
 *    opened, etc.).
 *  - Lay nodes out top→bottom with dagre, colour-code lineages, and
 *    render via @xyflow/react.
 *  - Click a node → call the parent's `onSelectNode(id)`, which previews
 *    that node's audio. A click does not move the head (#522): the head
 *    ring stays on the head and the previewed node is marked apart from
 *    it. Only "Set as head" moves the head.
 *  - Right-click a node → context menu with Set-as-head / Compare /
 *    Rename / Delete. Set-as-head is handed to the parent's
 *    `onSetHead`, because moving the head is the parent's to do and to
 *    record on the undo path (#453); only Rename calls the bridge
 *    (`rename_node`). Compare is enabled when the parent provides
 *    `onCompareNodes`. Delete is disabled — the content-addressed DAG
 *    has no safe delete semantics yet.
 *
 * The component doesn't own any session-level state — that lives in
 * the parent's `useSession` hook. We expose only what the parent
 * needs to wire up: a `head` and a `preview` for highlighting and an
 * `onSelectNode` for click handling.
 *
 * The graph is content-addressed, so it records which states exist, not
 * the order the user visited them. The parent can pass that order as
 * `path`: edges the user walked are drawn in the head ring's blue, and a
 * step the parent edges do not contain (an edit that returned to an
 * earlier state) is drawn as a dashed extra edge (#398). The layout
 * stays parent-only, so the path never moves a node.
 */

import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Background,
  Controls,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeMouseHandler,
  type NodeProps,
} from "@xyflow/react";

import "@xyflow/react/dist/style.css";

import {
  colourLineage,
  formatRelative,
  layoutDagre,
  nodeLabel,
  pathSteps,
} from "../lib/graph";
import {
  getGraph as bridgeGetGraph,
  renameNode as bridgeRenameNode,
  type GraphNode,
  type GraphSummary,
  type NodeId,
} from "../lib/tauri-bridge";

export interface GraphViewProps {
  /** The session's head node id; rendered with a highlight ring. */
  head: NodeId | null;
  /**
   * The node being previewed, if it is not the head: drawn with a dashed
   * outline of its own, so it reads as "looking at" and not as "on" (#522).
   * The head keeps its ring either way.
   */
  preview?: NodeId | null;
  /**
   * Called when the user clicks a node. The parent previews it. This is
   * not a head move and must not become one: the head, and the path undo
   * follows, change only through `onSetHead` (#522).
   */
  onSelectNode: (id: NodeId) => void;
  /**
   * M26: called when the user chooses "Compare with…" from the context
   * menu. The parent sets up compare mode with `head` as A and the
   * right-clicked `nodeId` as B.
   */
  onCompareNodes?: (nodeId: NodeId) => void;
  /**
   * The user chose "Set as head". The parent moves the session head
   * (`set_head_to`) and records the step on the path, so undo comes back
   * from it. GraphView does not move the head, because a move made here
   * is one undo does not know about (#453). A rejection is shown in the
   * graph's error banner.
   */
  onSetHead: (id: NodeId) => Promise<void>;
  /**
   * Name or un-name a node. The parent puts it in line with the session's
   * other edits (#498); without it the bridge is called directly.
   */
  onRenameNode?: (id: NodeId, label: string) => Promise<void>;
  /** Bumping this number forces a refetch of `get_graph`. The parent
   *  bumps it on `node-created` events so the graph stays in sync
   *  with the agent's edits. */
  refreshKey?: number;
  /**
   * The heads the user has been on, oldest first, ending at `head`: the
   * path taken. Drawn over the edges; absent or short, nothing is drawn.
   */
  path?: readonly NodeId[];
}

/**
 * The head ring's accent, so the path and the head read as one thing.
 * An inline `style.stroke` resolves `var()`, which a canvas colour (the
 * background below) would not.
 */
const PATH_STROKE = "var(--accent)";

/** An edge the user has not walked. */
const EDGE_STROKE = "var(--border-strong)";

/**
 * The background's dots: `--border`'s value, as a literal, because React
 * Flow takes the colour as a prop and not through the cascade.
 */
const GRID_COLOUR = "#26262b";

interface NodeData extends Record<string, unknown> {
  bubble: GraphNode;
  colour: string;
  isHead: boolean;
  /** Previewed, and not the head: the head ring is the head's alone. */
  isPreview: boolean;
  onContextMenu: (e: React.MouseEvent, id: string) => void;
}

/**
 * Single bubble. Memoised so re-renders driven by hover/zoom in
 * react-flow don't re-render every node body.
 */
const GraphBubble = memo(function GraphBubble({ data }: NodeProps) {
  const { bubble, colour, isHead, isPreview, onContextMenu } = data as NodeData;
  const label = nodeLabel(bubble);
  const tool = bubble.tool ?? "—";
  const when = formatRelative(bubble.created_at);

  return (
    <div
      data-testid="graph-bubble"
      data-node-id={bubble.id}
      data-is-head={isHead ? "true" : "false"}
      data-is-preview={isPreview ? "true" : "false"}
      onContextMenu={(e) => onContextMenu(e, bubble.id)}
      // Motion (#211). Branching is the product's core idea and this
      // view was silent about it: a new node appeared fully formed and
      // the head ring jumped between bubbles, so neither "something was
      // added" nor "you are now here" was ever shown.
      //
      // `node-in` runs on mount. That is once per node rather than once
      // per render because this component is memoised by node id and
      // React Flow keeps mounted nodes across pan and zoom — so
      // existing bubbles stay still while a newly appended one arrives.
      //
      // `transition` (the vocabulary's --dur-1) is on the ring, so the
      // head moving reads as a move rather than as two separate
      // redraws.
      className={
        "node-in transition min-w-[160px] max-w-[200px] rounded-lg border bg-[var(--surface-elev)] px-3 py-2 text-xs text-[var(--text)] shadow " +
        (isHead
          ? "ring-2 ring-[color:var(--accent)] border-[var(--accent)]"
          : isPreview
            ? // Dashed, so it differs from the head's solid ring by shape
              // and not by colour alone (#522).
              "outline-2 outline-dashed outline-offset-2 outline-[color:var(--accent)] border-[var(--accent-border)]"
            : "border-[var(--border-strong)]")
      }
      style={{ borderLeft: `4px solid ${colour}` }}
    >
      <Handle type="target" position={Position.Top} className="!bg-[var(--border-input)]" />
      <div
        className="truncate font-medium text-[var(--text)]"
        title={label}
      >
        {label}
      </div>
      <div className="mt-0.5 flex items-center justify-between gap-2 text-[10px] text-[var(--text-dim)]">
        <span className="truncate font-mono">{tool}</span>
        <span className="shrink-0">{when}</span>
      </div>
      <Handle type="source" position={Position.Bottom} className="!bg-[var(--border-input)]" />
    </div>
  );
});

const NODE_TYPES = { graphBubble: GraphBubble };

/** The zoom controls, through the variables React Flow reads them from. */
const CONTROLS_STYLE = {
  "--xy-controls-button-background-color": "var(--surface-elev-2)",
  "--xy-controls-button-background-color-hover": "var(--border-strong)",
  "--xy-controls-button-color": "var(--text-dim)",
  "--xy-controls-button-color-hover": "var(--text)",
  "--xy-controls-button-border-color": "var(--border-strong)",
} as React.CSSProperties;

interface ContextMenuState {
  x: number;
  y: number;
  nodeId: string;
}

export function GraphView({
  head,
  preview = null,
  onSelectNode,
  onCompareNodes,
  onSetHead,
  onRenameNode = bridgeRenameNode,
  refreshKey = 0,
  path,
}: GraphViewProps) {
  const [graph, setGraph] = useState<GraphSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [localRefresh, setLocalRefresh] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);

  const refetch = useCallback(() => {
    setLocalRefresh((n) => n + 1);
  }, []);

  // Fetch on mount + whenever `refreshKey` or `localRefresh` changes.
  useEffect(() => {
    let cancelled = false;
    bridgeGetGraph()
      .then((g) => {
        if (!cancelled) {
          setGraph(g);
          setError(null);
        }
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [refreshKey, localRefresh]);

  const handleSetHead = useCallback(
    async (nodeId: string) => {
      try {
        await onSetHead(nodeId);
        refetch();
      } catch (e) {
        setError(String(e));
      }
    },
    [onSetHead, refetch],
  );

  const handleRename = useCallback(
    async (nodeId: string, label: string) => {
      try {
        await onRenameNode(nodeId, label);
        refetch();
      } catch (e) {
        setError(String(e));
      }
    },
    [onRenameNode, refetch],
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, nodeId: string) => {
      e.preventDefault();
      const rect = containerRef.current?.getBoundingClientRect();
      const x = rect ? e.clientX - rect.left : e.clientX;
      const y = rect ? e.clientY - rect.top : e.clientY;
      setMenu({ x, y, nodeId });
    },
    [],
  );

  // Memoised lineage colours + dagre layout. Re-runs only when the
  // node list identity changes (i.e. after a refetch produced new
  // data) — react-flow's pan/zoom does not invalidate this.
  const { nodes, edges } = useMemo(() => {
    const summary = graph;
    if (!summary || summary.nodes.length === 0) {
      return { nodes: [] as Node[], edges: [] as Edge[] };
    }
    const colours = colourLineage(summary.nodes);
    const layout = layoutDagre(summary.nodes, {
      nodeWidth: 180,
      nodeHeight: 64,
    });

    const rfNodes: Node[] = summary.nodes.map((n) => {
      const pos = layout.positions.get(n.id) ?? { x: 0, y: 0 };
      const data: NodeData = {
        bubble: n,
        colour: colours.get(n.id) ?? "#888",
        isHead: head === n.id,
        isPreview: preview === n.id && head !== n.id,
        onContextMenu: handleContextMenu,
      };
      return {
        id: n.id,
        type: "graphBubble",
        data,
        position: pos,
      };
    });

    const ids = new Set(summary.nodes.map((n) => n.id));
    const steps = path ? pathSteps(summary.nodes, path) : [];
    const walked = new Set(
      steps.filter((s) => s.onTree).map((s) => `${s.source}->${s.target}`),
    );
    const markerEnd = { type: MarkerType.ArrowClosed, width: 14, height: 14 };
    const rfEdges: Edge[] = summary.nodes
      .filter((n) => n.parent && ids.has(n.parent))
      .map((n) => {
        const id = `${n.parent}->${n.id}`;
        const edge = { id, source: n.parent as string, target: n.id, markerEnd };
        return walked.has(id)
          ? { ...edge, className: "graph-path-step", style: { stroke: PATH_STROKE } }
          : { ...edge, style: { stroke: EDGE_STROKE } };
      });
    // A step the parent edges do not hold: an edit that came back to a
    // state reached before.
    for (const step of steps) {
      if (step.onTree) continue;
      rfEdges.push({
        id: step.id,
        source: step.source,
        target: step.target,
        markerEnd,
        className: "graph-path-step",
        style: { stroke: PATH_STROKE, strokeDasharray: "4 3" },
      });
    }

    return { nodes: rfNodes, edges: rfEdges };
  }, [graph, head, preview, handleContextMenu, path]);

  const handleNodeClick = useCallback<NodeMouseHandler>(
    (_e, node) => {
      onSelectNode(node.id);
    },
    [onSelectNode],
  );

  const dismissMenu = useCallback(() => setMenu(null), []);

  // Close the context menu on any click outside it (and on Escape).
  useEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && target.closest('[data-testid="graph-context-menu"]')) return;
      dismissMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismissMenu();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu, dismissMenu]);

  const startRename = useCallback((nodeId: string) => {
    setRenaming(nodeId);
    setMenu(null);
  }, []);

  return (
    <div
      ref={containerRef}
      data-testid="graph-view-root"
      // Fades in when the tab is chosen. Opacity only: React Flow
      // measures its container and must not see it move.
      className="fade-in relative h-full w-full bg-[var(--surface)]"
    >
      {error ? (
        <div
          role="alert"
          data-testid="graph-error"
          className="absolute left-3 top-3 z-20 rounded-md border border-[var(--danger-border)] bg-[var(--danger-soft)] px-3 py-2 text-xs text-[var(--danger)]"
        >
          {error}
        </div>
      ) : null}
      {graph && graph.nodes.length === 0 ? (
        <div
          data-testid="graph-empty"
          className="flex h-full w-full items-center justify-center text-sm text-[var(--text-dim)]"
        >
          No nodes yet — start a chat turn to build the session graph.
        </div>
      ) : (
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          onNodeClick={handleNodeClick}
          fitView
          minZoom={0.2}
          maxZoom={2}
          proOptions={{ hideAttribution: true }}
        >
          <Background gap={24} color={GRID_COLOUR} />
          <Controls
            position="bottom-right"
            showInteractive={false}
            // React Flow draws these buttons light unless told otherwise.
            style={CONTROLS_STYLE}
          />
        </ReactFlow>
      )}

      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={dismissMenu}
          onSetHead={() => {
            void handleSetHead(menu.nodeId);
          }}
          onRename={() => startRename(menu.nodeId)}
          onCompare={
            onCompareNodes
              ? () => {
                  onCompareNodes(menu.nodeId);
                  dismissMenu();
                }
              : undefined
          }
        />
      ) : null}

      {renaming ? (
        <RenameOverlay
          nodeId={renaming}
          onSubmit={async (value) => {
            await handleRename(renaming, value);
          }}
          onClose={() => setRenaming(null)}
        />
      ) : null}
    </div>
  );
}

interface ContextMenuProps {
  x: number;
  y: number;
  onClose: () => void;
  onSetHead: () => void;
  onRename: () => void;
  /** M26: when provided, the "Compare with…" item is enabled. */
  onCompare?: () => void;
}

/**
 * Right-click menu. Set-as-head is handed to the parent (`onSetHead`,
 * #453) and Rename is wired to the M24 IPC command `rename_node`.
 * Compare is enabled when the parent provides `onCompare`. Delete
 * remains disabled — the content-addressed DAG has no clean delete
 * semantics (a node may be a parent of others), so honouring "delete"
 * without orphaning descendants would need a follow-up GC pass.
 */
function ContextMenu({ x, y, onClose, onSetHead, onRename, onCompare }: ContextMenuProps) {
  const item = (
    label: string,
    enabled: boolean,
    onClick: () => void,
    title?: string,
  ) => (
    <button
      type="button"
      role="menuitem"
      disabled={!enabled}
      title={title}
      onClick={() => {
        onClick();
        onClose();
      }}
      className="block w-full px-3 py-1.5 text-left text-xs text-[var(--text)] hover:bg-[var(--surface-elev-2)] disabled:cursor-not-allowed disabled:text-[var(--text-faint)]"
    >
      {label}
    </button>
  );

  return (
    <div
      data-testid="graph-context-menu"
      role="menu"
      style={{ left: x, top: y }}
      className="absolute z-30 w-44 overflow-hidden rounded-md border border-[var(--border-strong)] bg-[var(--surface-elev)] py-1 shadow-(--shadow-pop)"
    >
      {item("Set as head", true, onSetHead)}
      {item(
        "Compare with…",
        !!onCompare,
        onCompare ?? (() => undefined),
        onCompare ? undefined : "open a second node to enable",
      )}
      {item("Rename", true, onRename)}
      {item(
        "Delete",
        false,
        () => undefined,
        "delete is not yet supported (content-addressed DAG)",
      )}
    </div>
  );
}

interface RenameOverlayProps {
  nodeId: string;
  onSubmit: (label: string) => void | Promise<void>;
  onClose: () => void;
}

/**
 * Tiny in-place input. Submit calls `rename_node` via the parent's
 * `onSubmit` handler; empty input clears the label.
 */
/**
 * Exported so its behaviour is testable directly (#253). The overlay
 * sits inside a react-flow canvas that jsdom cannot mount, and the
 * failure this guards against is entirely in the form.
 */
export function RenameOverlay({ nodeId, onSubmit, onClose }: RenameOverlayProps) {
  const [value, setValue] = useState("");
  return (
    <div
      data-testid="graph-rename-overlay"
      className="absolute inset-0 z-40 flex items-center justify-center bg-black/40"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <form
        className="flex flex-col gap-2 rounded-md border border-[var(--border-strong)] bg-[var(--surface-elev)] p-3 text-xs text-[var(--text)] shadow-(--shadow-pop)"
        onSubmit={(e) => {
          e.preventDefault();
          const name = value.trim();
          if (!name) return;
          void Promise.resolve(onSubmit(name)).finally(onClose);
        }}
      >
        <label
          htmlFor="rename-input"
          className="font-mono text-[10px] text-[var(--text-dim)]"
        >
          rename {nodeId.slice(0, 7)}
        </label>
        <input
          id="rename-input"
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="new label"
          className="field px-2 py-1 text-xs outline-none"
        />
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="btn-secondary rounded px-2 py-1"
          >
            Cancel
          </button>
          {/*
            Gated on a non-empty name, not hard-disabled (#253).

            This carried a bare `disabled` attribute and the tooltip
            "available after M24 lands" long after M24 landed —
            `rename_node` is registered, the bridge exports it, and this
            component's own doc comment says the overlay is wired to it.

            A disabled default submit button also suppresses implicit
            form submission, so Enter in the input did nothing either:
            with Cancel being `type="button"`, the form had no reachable
            submit path at all. Someone could type a name and never
            commit it.
          */}
          <button
            type="submit"
            disabled={!value.trim()}
            title={
              value.trim() ? undefined : "enter a name first"
            }
            className="btn-primary px-2 py-1"
          >
            Save
          </button>
        </div>
      </form>
    </div>
  );
}
