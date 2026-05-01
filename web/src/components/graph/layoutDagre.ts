import dagre from '@dagrejs/dagre'
import type { Edge, Node } from '@xyflow/react'

export const NODE_WIDTH = 220
export const NODE_HEIGHT = 90

export type LayoutDirection = 'TB' | 'LR'

export function layoutWithDagre<T extends Record<string, unknown>>(
  nodes: Node<T>[],
  edges: Edge[],
  direction: LayoutDirection = 'TB',
): Node<T>[] {
  const g = new dagre.graphlib.Graph()
  g.setDefaultEdgeLabel(() => ({}))
  g.setGraph({ rankdir: direction, nodesep: 40, ranksep: 80 })

  for (const node of nodes) {
    g.setNode(node.id, { width: NODE_WIDTH, height: NODE_HEIGHT })
  }
  for (const edge of edges) {
    g.setEdge(edge.source, edge.target)
  }

  dagre.layout(g)

  return nodes.map((node) => {
    const { x, y } = g.node(node.id)
    return {
      ...node,
      position: { x: x - NODE_WIDTH / 2, y: y - NODE_HEIGHT / 2 },
    }
  })
}
