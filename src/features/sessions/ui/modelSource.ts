import { createContext, useContext } from "react";
import {
  catalogStatus,
  findModel,
  modelsFor,
  resolveModel,
  type AgentModel,
  type CatalogStatus,
} from "../model/models";
import type { HarnessId } from "../model/session";
import {
  hasProbedHarnessAvailability,
  isHarnessAvailable,
  probeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";

/** Where the model picker gets its models and provider availability. The
 * default is this computer's catalog; a remote session supplies its host's. */
export type ModelSource = {
  /** Present only for non-local sources; local sources use global stores. */
  id?: string;
  modelsFor(harness: HarnessId): AgentModel[];
  resolve(harness: HarnessId, id?: string): AgentModel;
  find(id: string): AgentModel | undefined;
  available(harness: HarnessId): boolean;
  probed(): boolean;
  /** Refresh availability and catalogs, when the source supports it. */
  refresh(harnesses: HarnessId[]): void;
  /**
   * Whether a provider's models were read from its CLI. A source without it
   * (a remote host) lists what it has; the local one lists only what was read.
   */
  catalog?(harness: HarnessId): CatalogStatus;
};

export const LOCAL_MODEL_SOURCE: ModelSource = {
  modelsFor,
  resolve: resolveModel,
  find: findModel,
  available: isHarnessAvailable,
  probed: hasProbedHarnessAvailability,
  refresh: (harnesses) => {
    void probeHarnessAvailability();
    void refreshHarnessCatalogs(harnesses);
  },
  catalog: catalogStatus,
};

/** The models the picker lists for a provider: only ones read from its CLI. */
export function pickerModels(
  source: ModelSource,
  harness: HarnessId,
): AgentModel[] {
  const status = source.catalog?.(harness);
  return !status || status.status === "live" ? source.modelsFor(harness) : [];
}

/** A model the picker may list: its provider's catalog was read and has it. */
export function isPickable(source: ModelSource, model: AgentModel): boolean {
  return pickerModels(source, model.harness).some(
    (entry) => entry.id === model.id,
  );
}

export const ModelSourceContext =
  createContext<ModelSource>(LOCAL_MODEL_SOURCE);

export const useModelSource = () => useContext(ModelSourceContext);
