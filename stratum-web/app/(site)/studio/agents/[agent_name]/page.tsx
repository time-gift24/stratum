import { AgentEditor } from "@/components/stratum/studio/agent-editor"

export default async function AgentPage({
  params,
}: {
  params: Promise<{ agent_name: string }>
}) {
  const { agent_name: agentName } = await params
  return <AgentEditor key={agentName} agentName={agentName} />
}
