import { execFile } from "node:child_process"
import { mkdtemp, readFile, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, resolve } from "node:path"
import { promisify } from "node:util"
import { fileURLToPath } from "node:url"

const execFileAsync = promisify(execFile)
const scriptDirectory = dirname(fileURLToPath(import.meta.url))
const webRoot = resolve(scriptDirectory, "..")
const repositoryRoot = resolve(webRoot, "..")
const committedArtifact = resolve(repositoryRoot, "CONTEXT.html")
const temporaryDirectory = await mkdtemp(
  resolve(tmpdir(), "stratum-context-site-")
)
const generatedArtifact = resolve(temporaryDirectory, "CONTEXT.html")

try {
  await execFileAsync(
    process.execPath,
    [resolve(scriptDirectory, "generate.mts"), "--output", generatedArtifact],
    {
      cwd: webRoot,
    }
  )
  const [committed, generated] = await Promise.all([
    readFile(committedArtifact),
    readFile(generatedArtifact),
  ])
  if (!committed.equals(generated)) {
    throw new Error(
      "CONTEXT.html is stale; run `pnpm build:context-site` and commit the generated artifact"
    )
  }
  console.log("CONTEXT.html matches the typed context-site sources")
} finally {
  await rm(temporaryDirectory, { force: true, recursive: true })
}
