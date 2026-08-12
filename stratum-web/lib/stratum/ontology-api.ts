// Ontology hooks share one API resolver. Tests can inject an implementation;
// production always uses the configured Stratum backend and never seeded data.
import {
  createStratumApi,
  STRATUM_API_BASE_URL,
  type StratumApi,
} from "@/lib/stratum/api"

export function resolveOntologyApi(
  apiOption: StratumApi | undefined
): StratumApi {
  if (apiOption !== undefined) return apiOption
  return createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
}
