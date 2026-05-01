import { useMemo } from 'react'
import {
  Background,
  Controls,
  MarkerType,
  ReactFlow,
  type Edge,
  type Node,
  type NodeTypes,
} from '@xyflow/react'

import type { components } from '#/api'
import { TaskNode, type TaskNodeData } from './TaskNode'
import { layoutWithDagre } from './layoutDagre'

import '@xyflow/react/dist/style.css'

type Task = components['schemas']['TaskResponse']

const nodeTypes: NodeTypes = { task: TaskNode }

interface TaskGraphProps {
  tasks: Task[]
  selectedId: number | null
  onSelect: (id: number | null) => void
  theme: 'light' | 'dark'
}

export function TaskGraph({
  tasks,
  selectedId,
  onSelect,
  theme,
}: TaskGraphProps) {
  const { nodes, edges } = useMemo(() => {
    const ids = new Set(tasks.map((t) => t.id))
    const rawNodes: Node<TaskNodeData>[] = tasks.map((task) => ({
      id: String(task.id),
      type: 'task',
      data: { task },
      position: { x: 0, y: 0 },
      selected: task.id === selectedId,
    }))
    const rawEdges: Edge[] = []
    for (const task of tasks) {
      for (const dep of task.dependencies) {
        if (!ids.has(dep)) continue
        rawEdges.push({
          id: `${task.id}-${dep}`,
          source: String(task.id),
          target: String(dep),
          markerEnd: { type: MarkerType.ArrowClosed },
        })
      }
    }
    const positioned = layoutWithDagre(rawNodes, rawEdges, 'TB')
    return { nodes: positioned, edges: rawEdges }
  }, [tasks, selectedId])

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      fitView
      nodesDraggable
      nodesConnectable={false}
      elementsSelectable
      colorMode={theme}
      onNodeClick={(_e, n) => onSelect(Number(n.id))}
      onPaneClick={() => onSelect(null)}
      proOptions={{ hideAttribution: false }}
    >
      <Background />
      <Controls />
    </ReactFlow>
  )
}
