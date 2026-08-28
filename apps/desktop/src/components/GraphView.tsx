/**
 * GraphView — react-flow visualisation of the session DAG.
 *
 * Responsibilities:
 *  - Fetch the full graph via the `get_graph` bridge call on mount and
 *    whenever the parent signals a refresh (new node created, project
 *    opened, etc.).
 *  - Lay nodes out top→bottom with dagre, colour-code lineages, and
 *    render via @xyflow/react.
 *  - Click a node → call the parent's `onSelectNode(id)` so the canvas
 *    pane re-renders that node's audio.
 *  - Right-click a node → context menu with Set-as-head / Compare /
 *    Rename / Delete. Set-as-head and Rename are wired to the M24 IPC
 *    commands `set_head_to` / `rename_node`. Compare is enabled when
 *    the parent provides `onCompareNodes`. Delete is disabled — the
 *    content-addressed DAG has no safe delete semantics yet.
 *
 * The component doesn't own any session-level state — that lives in
 * the parent's `useSession` hook. We expose only what the parent
 * needs to wire up: a `head` for highlighting and an `onSelectNode`
 * for click handling.
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
} from "../lib/graph";
import {
  getGraph as bridgeGetGraph,
  renameNode as bridgeRenameNode,
  setHeadTo as bridgeSetHeadTo,
  type GraphNode,
  type GraphSummary,
  type NodeId,
} from "../lib/tauri-bridge";

export interface GraphViewProps {
  /** Currently-selected head node id; rendered with a highlight ring. */
  head: NodeId | null;
  /** Called when the user clicks a node. The parent should update
   *  its head pointer and re-render the canvas. */
  onSelectNode: (id: NodeId) => void;
  /**
   * M26: called when the user chooses "Compare with…" from the context
   * menu. The parent sets up compare mode with `head` as A and the
   * right-clicked `nodeId` as B.
   */
  onCompareNodes?: (nodeId: NodeId) => void;
  /** Bumping this number forces a refetch of `get_graph`. The parent
   *  bumps it on `node-created` events so the graph stays in sync
   *  with the agent's edits. */
  refreshKey?: number;
}

interface NodeData extends Record<string, unknown> {
  bubble: GraphNode;
  colour: string;
  isHead: boolean;
  onContextMenu: (e: React.MouseEvent, id: string) => void;
}

/**
 * Single bubble. Memoised so re-renders driven by hover/zoom in
 * react-flow don't re-render every node body.
 */
const GraphBubble = memo(function GraphBubble({ data }: NodeProps) {
  const { bubble, colour, isHead, onContextMenu } = data as NodeData;
  const label = nodeLabel(bubble);
  const tool = bubble.tool ?? "—";
  const when = formatRelative(bubble.created_at);

  return (
    <div
      data-testid="graph-bubble"
      data-node-id={bubble.id}
      data-is-head={isHead ? "true" : "false"}
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
        "node-in transition min-w-[160px] max-w-[200px] rounded-lg border bg-neutral-900 px-3 py-2 text-xs text-neutral-100 shadow " +
        (isHead
          ? "ring-2 ring-blue-500 border-blue-500"
          : "border-neutral-700")
      }
      style={{ borderLeft: `4px solid ${colour}` }}
    >
      <Handle type="target" position={Position.Top} className="!bg-neutral-600" />
      <div
        className="truncate font-medium text-neutral-100"
        title={label}
      >
        {label}
      </div>
      <div className="mt-0.5 flex items-center justify-between gap-2 text-[10px] text-neutral-400">
        <span className="truncate font-mono">{tool}</span>
        <span className="shrink-0">{when}</span>
      </div>
      <Handle type="source" position={Position.Bottom} className="!bg-neutral-600" />
    </div>
  );
});

const NODE_TYPES = { graphBubble: GraphBubble };

interface ContextMenuState {
  x: number;
  y: number;
  nodeId: string;
}

export function GraphView({ head, onSelectNode, onCompareNodes, refreshKey = 0 }: GraphViewProps) {
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
        await bridgeSetHeadTo(nodeId);
        refetch();
      } catch (e) {
        setError(String(e));
      }
    },
    [refetch],
  );

  const handleRename = useCallback(
    async (nodeId: string, label: string) => {
      try {
        await bridgeRenameNode(nodeId, label);
        refetch();
      } catch (e) {
        setError(String(e));
      }
    },
    [refetch],
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
    const rfEdges: Edge[] = summary.nodes
      .filter((n) => n.parent && ids.has(n.parent))
      .map((n) => ({
        id: `${n.parent}->${n.id}`,
        source: n.parent as string,
        target: n.id,
        markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14 },
        style: { stroke: "#52525b" },
      }));

    return { nodes: rfNodes, edges: rfEdges };
  }, [graph, head, handleContextMenu]);

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
      className="relative h-full w-full bg-neutral-900"
    >
      {error ? (
        <div
          role="alert"
          data-testid="graph-error"
          className="absolute left-3 top-3 z-20 rounded-md border border-red-800 bg-red-900/60 px-3 py-2 text-xs text-red-100"
        >
          {error}
        </div>
      ) : null}
      {graph && graph.nodes.length === 0 ? (
        <div
          data-testid="graph-empty"
          className="flex h-full w-full items-center justify-center text-sm text-neutral-400"
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
          <Background gap={24} color="#27272a" />
          <Controls
            position="bottom-right"
            showInteractive={false}
            className="!bg-neutral-800 !text-neutral-100"
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
 * Right-click menu. Set-as-head and Rename are wired to the M24 IPC
 * commands `set_head_to` and `rename_node`. Compare is enabled when
 * the parent provides `onCompare`. Delete remains disabled — the
 * content-addressed DAG has no clean delete semantics (a node may
 * be a parent of others), so honouring "delete" without orphaning
 * descendants would need a follow-up GC pass.
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
      className="block w-full px-3 py-1.5 text-left text-xs text-neutral-100 hover:bg-neutral-800 disabled:cursor-not-allowed disabled:text-neutral-500"
    >
      {label}
    </button>
  );

  return (
    <div
      data-testid="graph-context-menu"
      role="menu"
      style={{ left: x, top: y }}
      className="absolute z-30 w-44 overflow-hidden rounded-md border border-neutral-700 bg-neutral-900 py-1 shadow-lg"
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
        className="flex flex-col gap-2 rounded-md border border-neutral-700 bg-neutral-900 p-3 text-xs text-neutral-100"
        onSubmit={(e) => {
          e.preventDefault();
          const name = value.trim();
          if (!name) return;
          void Promise.resolve(onSubmit(name)).finally(onClose);
        }}
      >
        <label
          htmlFor="rename-input"
          className="font-mono text-[10px] text-neutral-400"
        >
          rename {nodeId.slice(0, 7)}
        </label>
        <input
          id="rename-input"
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="new label"
          className="rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-xs outline-none focus:border-blue-600"
        />
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-neutral-700 px-2 py-1 hover:bg-neutral-800"
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
            className="rounded bg-blue-600 px-2 py-1 text-white disabled:cursor-not-allowed disabled:opacity-50"
          >
            Save
          </button>
        </div>
      </form>
    </div>
  );
}
