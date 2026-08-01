import { redirect } from "next/navigation"

/**
 * 根路径：产品是单对话页，/ 直接重定向到 /conversation。
 */
export default function RootPage() {
  redirect("/conversation")
}
