import { ScheduleHistory } from "@/components/stratum/scheduler/schedule-history"

export default async function SchedulePage({
  params,
}: {
  params: Promise<{ scheduleId: string }>
}) {
  const { scheduleId } = await params
  return <ScheduleHistory key={scheduleId} scheduleId={scheduleId} />
}
