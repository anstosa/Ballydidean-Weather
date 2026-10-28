import { constants as fsConstants } from "node:fs";
import {
  chown,
  link,
  lstat,
  mkdir,
  open,
  readdir,
  realpath,
  rename,
  statfs,
  unlink,
} from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";

// isolate cycle protocol internals within the deployed single source of truth
const adjustmentCyclePageProtocol = (() => {
  const ADJUSTMENT_CYCLE_PAGE_CONTRACT_VERSION = "adjustment-cycle-page/v1";
  const ADJUSTMENT_CYCLE_ACK_CONTRACT_VERSION = "adjustment-cycle-page-ack/v1";
  const ADJUSTMENT_CYCLE_FINAL_MANIFEST_CONTRACT_VERSION =
    "adjustment-cycle-final-manifest/v1";
  const ADJUSTMENT_CYCLE_SEALED_FINAL_MANIFEST_CONTRACT_VERSION =
    "adjustment-cycle-final-manifest/v2";
  const ADJUSTMENT_CYCLE_CAPSULE_CONTRACT_VERSION = "adjustment-cycle-capsule/v2";
  const ADJUSTMENT_EVIDENCE_GAP_CONTRACT_VERSION = "adjustment-evidence-gap/v1";
  const ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES = 256 * 1_024;
  const ADJUSTMENT_CYCLE_MAXIMUM_PAGES = 19;
  const ADJUSTMENT_CYCLE_MAXIMUM_PAYLOAD_BYTES = 4_832 * 1_024;
  const ADJUSTMENT_DAILY_MAXIMUM_PAGES = 22;
  const ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS = 4_096;
  const ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS = 4;

  const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
  const DUE_KEY_PATTERN = /^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u;
  const LOCAL_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/u;
  const INSTANT_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u;
  const UINT64_MAXIMUM = 0xffff_ffff_ffff_ffffn;
  const CAPSULE_MAGIC = Buffer.from("WXACYCL2", "ascii");
  const CAPSULE_HEADER_BYTES = 16;
  const GAP_REASONS = new Set([
    "archive_refused",
    "capacity_refused",
    "page_limit",
    "projection_limit",
    "slot_unavailable",
    "transport_refused",
    "unacknowledged",
  ]);

  // create one empty bounded online page chain
  function createCyclePageState(input) {
    requireExactKeys(input, ["dailyPageCount", "dueKey", "generation", "localDate"], "cycle page state input");
    requireDueKey(input.dueKey);
    requireUint64(input.generation, "generation");
    requireLocalDate(input.localDate);

    // require the open daily page count below its exact ceiling
    if (!Number.isInteger(input.dailyPageCount) || input.dailyPageCount < 0 ||
      input.dailyPageCount >= ADJUSTMENT_DAILY_MAXIMUM_PAGES) {
      throw new TypeError("dailyPageCount is invalid");
    }

    return {
      contractVersion: "adjustment-cycle-page-state/v1",
      dueKey: input.dueKey,
      generation: input.generation,
      localDate: input.localDate,
      dailyPageCountAtOpen: input.dailyPageCount,
      pages: [],
      acknowledgements: [],
      gaps: [],
      slots: [],
      organicProjectionCount: 0,
      schedulerProjectionCount: 0,
      finalized: null,
    };
  }

  // append one predecessor-bound page after durable slot publication
  async function appendCyclePage(state, input, ports) {
    validateCyclePageState(state);
    requireExactKeys(input, ["payload", "projections"], "cycle page append");
    requireDurabilityPort(ports?.persistPage, "persistPage");
    const payload = requireBuffer(input.payload, "page payload");
    const projections = normalizeProjections(input.projections);
    const existingProjectionIdentities = new Set(state.pages.flatMap(
      // collect every already bound projection identity
      (page) => page.projectionIdentities,
    ));

    // prohibit changes after immutable finalization
    if (state.finalized !== null) {
      throw pageError("cycle_already_finalized");
    }

    // preserve only current and next crash slots
    if (state.slots.length >= 2) {
      throw pageError("slot_unavailable");
    }

    // enforce the transport payload ceiling
    if (payload.length === 0 || payload.length > ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES) {
      throw pageError("page_payload_refused");
    }

    // prohibit relabelling or duplicating one immutable projection
    if (projections.some((projection) =>
      existingProjectionIdentities.has(projection.identitySha256))) {
      throw pageError("projection_identity_collision");
    }

    // enforce exact cycle and local-date page ceilings
    if (state.pages.length >= ADJUSTMENT_CYCLE_MAXIMUM_PAGES ||
      state.dailyPageCountAtOpen + state.pages.length >= ADJUSTMENT_DAILY_MAXIMUM_PAGES) {
      throw pageError("page_limit");
    }

    const projectionCounts = countProjections(projections);
    const nextOrganicCount = state.organicProjectionCount + projectionCounts.organic;
    const nextSchedulerCount = state.schedulerProjectionCount + projectionCounts.scheduler;

    // retain four scheduler reservations before organic admission
    if (nextOrganicCount > ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS -
        ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS ||
      nextSchedulerCount > ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS ||
      nextOrganicCount + nextSchedulerCount > ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS) {
      throw pageError("projection_limit");
    }

    const previous = state.pages.at(-1) ?? null;
    const header = {
      contractVersion: ADJUSTMENT_CYCLE_PAGE_CONTRACT_VERSION,
      dueKey: state.dueKey,
      generation: state.generation,
      pageIndex: state.pages.length,
      predecessorPageSha256: previous?.pageSha256 ?? null,
      payloadSha256: sha256(payload),
      payloadOffset: previous === null
        ? 0
        : previous.payloadOffset + previous.payloadLength,
    };
    const pageSha256 = hashPage(header, payload);
    const slot = state.slots.length === 0 ? "current" : "next";
    const durable = await ports.persistPage({
      header,
      pageSha256,
      payload,
      projections,
      slot,
    });
    requireDurabilityProof(durable, "pageSha256", pageSha256);
    const page = {
      pageIndex: header.pageIndex,
      pageSha256,
      predecessorPageSha256: header.predecessorPageSha256,
      payloadSha256: header.payloadSha256,
      payloadOffset: header.payloadOffset,
      payloadLength: payload.length,
      projectionIdentities: projections.map(
        // retain only ordered immutable projection identities
        (projection) => projection.identitySha256,
      ),
      projectionChannels: projections.map(
        // retain the channel bound by the page payload
        (projection) => projection.channel,
      ),
    };
    return {
      ...state,
      pages: [...state.pages, page],
      slots: [...state.slots, { pageIndex: page.pageIndex, pageSha256, slot }],
      organicProjectionCount: nextOrganicCount,
      schedulerProjectionCount: nextSchedulerCount,
    };
  }

  // acknowledge the oldest page only after exact fsync proof
  async function ackCyclePage(state, input, ports) {
    validateCyclePageState(state);
    requireExactKeys(input, ["acknowledgedAt", "pageSha256"], "cycle page acknowledgement");
    requireSha256(input.pageSha256, "pageSha256");
    requireInstant(input.acknowledgedAt, "acknowledgedAt");
    requireDurabilityPort(ports?.persistAcknowledgement, "persistAcknowledgement");
    const oldest = state.slots[0];

    // require one exact in-order pending page
    if (oldest === undefined || oldest.pageSha256 !== input.pageSha256) {
      const existing = state.acknowledgements.find(
        // locate an exact idempotent prior acknowledgement
        (acknowledgement) => acknowledgement.pageSha256 === input.pageSha256,
      );

      // return only an exact prior acknowledgement
      if (existing !== undefined) {
        // reject retry with a different acknowledgement instant
        if (existing.acknowledgedAt !== input.acknowledgedAt) {
          throw pageError("acknowledgement_collision");
        }
        return state;
      }
      throw pageError("acknowledgement_order_refused");
    }

    const page = state.pages[oldest.pageIndex];
    const acknowledgement = {
      contractVersion: ADJUSTMENT_CYCLE_ACK_CONTRACT_VERSION,
      dueKey: state.dueKey,
      generation: state.generation,
      pageIndex: page.pageIndex,
      pageSha256: page.pageSha256,
      acknowledgedAt: input.acknowledgedAt,
    };
    const acknowledgementSha256 = sha256(canonicalJsonBytes(acknowledgement));
    const nextSlots = state.slots.slice(1).map(
      // move only an already durable next page into current
      (slotValue) => ({ ...slotValue, slot: "current" }),
    );
    const durable = await ports.persistAcknowledgement({
      acknowledgement,
      acknowledgementSha256,
      nextSlots,
    });
    requireDurabilityProof(durable, "acknowledgementSha256", acknowledgementSha256);
    return {
      ...state,
      acknowledgements: [...state.acknowledgements, {
        ...acknowledgement,
        acknowledgementSha256,
      }],
      slots: nextSlots,
    };
  }

  // finalize one closed acknowledged chain after durable publication
  async function finalizeCyclePages(state, input, ports) {
    validateCyclePageState(state);
    // preserve legacy fixture closure while requiring explicit sealed runtime closure
    requireExactKeys(input, Object.hasOwn(input, "inputSeal")
      ? ["finalizedAt", "inputSeal"] : ["finalizedAt"], "cycle finalization");
    requireInstant(input.finalizedAt, "finalizedAt");
    requireDurabilityPort(ports?.persistFinalManifest, "persistFinalManifest");

    // reuse only the already immutable final manifest
    if (state.finalized !== null) {
      // reject a changed finalization instant
      if (state.finalized.manifest.finalizedAt !== input.finalizedAt) {
        throw pageError("final_manifest_collision");
      }
      return { manifest: state.finalized.manifest, state };
    }

    // require at least one fully acknowledged page
    if (state.pages.length === 0 || state.slots.length !== 0 ||
      state.acknowledgements.length !== state.pages.length) {
      throw pageError("cycle_not_fully_acknowledged");
    }

    const pages = state.pages.map(
      // bind exact ordered page metadata without payload duplication
      (page) => ({
        pageIndex: page.pageIndex,
        pageSha256: page.pageSha256,
        predecessorPageSha256: page.predecessorPageSha256,
        payloadSha256: page.payloadSha256,
        payloadOffset: page.payloadOffset,
        payloadLength: page.payloadLength,
      }),
    );
    const acknowledgementHashes = state.acknowledgements.map(
      // bind exact durable acknowledgement order
      (acknowledgement) => acknowledgement.acknowledgementSha256,
    );
    const acknowledgedProjectionIdentities = state.pages.flatMap(
      // admit projections only from pages with durable acknowledgements
      (page) => page.projectionIdentities,
    ).sort();
    const gapHashes = state.gaps.map(
      // bind every durable categorical gap
      (gap) => gap.gapSha256,
    ).sort();
    const manifest = {
      contractVersion: input.inputSeal === undefined
        ? ADJUSTMENT_CYCLE_FINAL_MANIFEST_CONTRACT_VERSION
        : ADJUSTMENT_CYCLE_SEALED_FINAL_MANIFEST_CONTRACT_VERSION,
      dueKey: state.dueKey,
      generation: state.generation,
      finalizedAt: input.finalizedAt,
      pageCount: pages.length,
      totalPayloadBytes: pages.reduce(
        // sum exact acknowledged payload bytes
        (total, page) => total + page.payloadLength,
        0,
      ),
      pages,
      pageRootSha256: sha256(canonicalJsonBytes(pages)),
      acknowledgementRootSha256: sha256(canonicalJsonBytes(acknowledgementHashes)),
      acknowledgedProjectionCount: acknowledgedProjectionIdentities.length,
      acknowledgedProjectionRootSha256: sha256(canonicalJsonBytes(acknowledgedProjectionIdentities)),
      evidenceGapCount: gapHashes.length,
      evidenceGapRootSha256: sha256(canonicalJsonBytes(gapHashes)),
    };
    // bind the explicit combined-input seal into the immutable cold manifest
    if (input.inputSeal !== undefined) {
      manifest.inputSeal = input.inputSeal;
    }
    validateCycleFinalManifest(manifest);
    const manifestSha256 = sha256(canonicalJsonBytes(manifest));
    const durable = await ports.persistFinalManifest({ manifest, manifestSha256 });
    requireDurabilityProof(durable, "manifestSha256", manifestSha256);
    const nextState = { ...state, finalized: { manifest, manifestSha256 } };
    return { manifest, manifestSha256, state: nextState };
  }

  // reconcile one chain from exact durable pages and acknowledgements
  function reconcileCyclePages(input) {
    requireExactKeys(input, ["durable", "expected"], "cycle reconciliation");
    const expected = input.expected;
    validateCyclePageState(expected);
    requireExactKeys(input.durable, ["acknowledgements", "finalized", "gaps", "pages"], "durable cycle state");
    const durable = input.durable;

    // require arrays for every append-only durable class
    if (!Array.isArray(durable.pages) || !Array.isArray(durable.acknowledgements) ||
      !Array.isArray(durable.gaps)) {
      throw pageError("durable_cycle_state_invalid");
    }

    const durablePages = [];

    // rebuild every durable page against its durable predecessor
    for (let index = 0; index < durable.pages.length; index += 1) {
      durablePages.push(normalizePageMetadata(
        durable.pages[index],
        { ...expected, pages: durablePages },
        index,
      ));
    }
    const durableAcknowledgements = durable.acknowledgements.map(
      // validate and normalize one durable acknowledgement
      (acknowledgement, index) => normalizeDurableAcknowledgement(
        acknowledgement,
        expected,
        durablePages,
        index,
      ),
    );
    const durableGaps = durable.gaps.map(
      // validate one durable gap
      (gap) => validateEvidenceGap(gap, expected),
    );

    // reject any durable chain that conflicts with known local bytes
    if (!isPrefix(expected.pages, durablePages) && !isPrefix(durablePages, expected.pages)) {
      throw pageError("durable_page_collision");
    }

    // retain at most the current and next unacknowledged pages
    if (durablePages.length - durableAcknowledgements.length > 2) {
      throw pageError("hidden_page_spool_refused");
    }

    const slots = durablePages.slice(durableAcknowledgements.length).map(
      // assign only current and next physical slots
      (page, index) => ({
        pageIndex: page.pageIndex,
        pageSha256: page.pageSha256,
        slot: index === 0 ? "current" : "next",
      }),
    );
    const reconciled = {
      ...expected,
      pages: durablePages,
      acknowledgements: durableAcknowledgements,
      gaps: durableGaps,
      slots,
      organicProjectionCount: countPageChannels(durablePages, "organic"),
      schedulerProjectionCount: countPageChannels(durablePages, "scheduler_request"),
      finalized: durable.finalized,
    };
    validateCyclePageState(reconciled);
    return {
      state: reconciled,
      status: reconciled.finalized !== null
        ? "closed"
        : reconciled.slots.length === 0
          ? "ready"
          : "awaiting_acknowledgement",
    };
  }

  // record one durable categorical evidence gap
  async function recordEvidenceGap(state, input, ports) {
    validateCyclePageState(state);
    requireExactKeys(input, ["occurredAt", "projectionIdentitySha256", "reason"], "evidence gap input");
    requireInstant(input.occurredAt, "occurredAt");
    requireSha256(input.projectionIdentitySha256, "projectionIdentitySha256");

    // prohibit rewriting a closed cycle manifest
    if (state.finalized !== null) {
      throw pageError("cycle_already_finalized");
    }

    // require one closed refusal category
    if (!GAP_REASONS.has(input.reason)) {
      throw new TypeError("evidence gap reason is invalid");
    }
    requireDurabilityPort(ports?.persistGap, "persistGap");
    const gap = {
      contractVersion: ADJUSTMENT_EVIDENCE_GAP_CONTRACT_VERSION,
      dueKey: state.dueKey,
      generation: state.generation,
      projectionIdentitySha256: input.projectionIdentitySha256,
      reason: input.reason,
      occurredAt: input.occurredAt,
      qualificationDisposition: "forever_unqualified",
    };
    const gapSha256 = sha256(canonicalJsonBytes(gap));
    const existing = state.gaps.find(
      // find an exact prior gap for idempotent retry
      (candidate) => candidate.projectionIdentitySha256 === input.projectionIdentitySha256,
    );

    // reuse only byte-identical gap evidence
    if (existing !== undefined) {
      // reject relabelling one already gapped projection
      if (existing.gapSha256 !== gapSha256) {
        throw pageError("evidence_gap_collision");
      }
      return state;
    }

    const durable = await ports.persistGap({ gap, gapSha256 });
    requireDurabilityProof(durable, "gapSha256", gapSha256);
    return { ...state, gaps: [...state.gaps, { ...gap, gapSha256 }] };
  }

  // preserve normal serving while page admission records a durable gap
  async function appendCyclePageFailOpen(state, input, ports) {
    try {
      const nextState = await appendCyclePage(state, input, ports);
      return { state: nextState, status: "page_pending_acknowledgement", servingBlocked: false };
    } catch (error) {
      const projection = input.projections?.[0];

      // report an honest unrecorded gap when no stable identity exists
      if (projection === undefined || !SHA256_PATTERN.test(projection.identitySha256 ?? "")) {
        return {
          state,
          status: "evidence_gap_unrecorded",
          reason: classifyPageFailure(error),
          servingBlocked: false,
        };
      }

      try {
        const nextState = await recordEvidenceGap(state, {
          occurredAt: ports.now().toISOString(),
          projectionIdentitySha256: projection.identitySha256,
          reason: classifyPageFailure(error),
        }, ports);
        return { state: nextState, status: "evidence_gap", servingBlocked: false };
      } catch {
        return {
          state,
          status: "evidence_gap_persistence_refused",
          reason: classifyPageFailure(error),
          servingBlocked: false,
        };
      }
    }
  }

  // validate one complete in-memory page-chain state
  function validateCyclePageState(state) {
    requireExactKeys(state, [
      "contractVersion",
      "dueKey",
      "generation",
      "localDate",
      "dailyPageCountAtOpen",
      "pages",
      "acknowledgements",
      "gaps",
      "slots",
      "organicProjectionCount",
      "schedulerProjectionCount",
      "finalized",
    ], "cycle page state");

    // require the exact state contract
    if (state.contractVersion !== "adjustment-cycle-page-state/v1") {
      throw new TypeError("cycle page state contract is invalid");
    }
    requireDueKey(state.dueKey);
    requireUint64(state.generation, "generation");
    requireLocalDate(state.localDate);

    // enforce every bounded array and count
    if (!Array.isArray(state.pages) || state.pages.length > ADJUSTMENT_CYCLE_MAXIMUM_PAGES ||
      !Array.isArray(state.acknowledgements) ||
      !Array.isArray(state.gaps) || !Array.isArray(state.slots) || state.slots.length > 2 ||
      !Number.isInteger(state.dailyPageCountAtOpen) || state.dailyPageCountAtOpen < 0 ||
      state.dailyPageCountAtOpen + state.pages.length > ADJUSTMENT_DAILY_MAXIMUM_PAGES ||
      !Number.isInteger(state.organicProjectionCount) || state.organicProjectionCount < 0 ||
      !Number.isInteger(state.schedulerProjectionCount) || state.schedulerProjectionCount < 0) {
      throw new TypeError("cycle page state bounds are invalid");
    }

    // validate the complete predecessor and offset chain
    for (let index = 0; index < state.pages.length; index += 1) {
      normalizePageMetadata(state.pages[index], state, index);
    }

    // validate every acknowledgement prefix
    for (let index = 0; index < state.acknowledgements.length; index += 1) {
      normalizeDurableAcknowledgement(state.acknowledgements[index], state, state.pages, index);
    }

    // require acknowledgements to form one prefix
    if (state.acknowledgements.length > state.pages.length ||
      state.slots.length !== state.pages.length - state.acknowledgements.length) {
      throw new TypeError("cycle acknowledgement frontier is invalid");
    }

    // validate exact current and next slot projections
    for (let index = 0; index < state.slots.length; index += 1) {
      const slot = state.slots[index];
      requireExactKeys(slot, ["pageIndex", "pageSha256", "slot"], "cycle page slot");
      const pageIndex = state.acknowledgements.length + index;

      // reject hidden or reordered slot state
      if (slot.pageIndex !== pageIndex || slot.pageSha256 !== state.pages[pageIndex].pageSha256 ||
        slot.slot !== (index === 0 ? "current" : "next")) {
        throw new TypeError("cycle page slot is invalid");
      }
    }

    const projectionIdentities = state.pages.flatMap(
      // collect every retained projection identity
      (page) => page.projectionIdentities,
    );
    const uniqueProjectionIdentities = new Set(projectionIdentities);
    const organicProjectionCount = countPageChannels(state.pages, "organic");
    const schedulerProjectionCount = countPageChannels(state.pages, "scheduler_request");

    // require exact counts, uniqueness and reserved scheduler bounds
    if (uniqueProjectionIdentities.size !== projectionIdentities.length ||
      state.organicProjectionCount !== organicProjectionCount ||
      state.schedulerProjectionCount !== schedulerProjectionCount ||
      organicProjectionCount > ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS -
        ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS ||
      schedulerProjectionCount > ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS) {
      throw new TypeError("cycle projection frontier is invalid");
    }

    // validate every durable evidence gap
    for (const gap of state.gaps) {
      validateEvidenceGap(gap, state);
    }

    // validate immutable closure when present
    if (state.finalized !== null) {
      requireExactKeys(state.finalized, ["manifest", "manifestSha256"], "finalized cycle");
      requireSha256(state.finalized.manifestSha256, "finalized manifestSha256");
      validateCycleFinalManifest(state.finalized.manifest);

      // bind the retained final manifest bytes
      if (sha256(canonicalJsonBytes(state.finalized.manifest)) !== state.finalized.manifestSha256) {
        throw new TypeError("finalized manifest hash is invalid");
      }

      const manifest = state.finalized.manifest;
      const projectedPages = state.pages.map(
        // project the exact final page metadata
        (page) => ({
          pageIndex: page.pageIndex,
          pageSha256: page.pageSha256,
          predecessorPageSha256: page.predecessorPageSha256,
          payloadSha256: page.payloadSha256,
          payloadOffset: page.payloadOffset,
          payloadLength: page.payloadLength,
        }),
      );
      const acknowledgementHashes = state.acknowledgements.map(
        // bind the exact acknowledgement prefix
        (acknowledgement) => acknowledgement.acknowledgementSha256,
      );
      const acknowledgedProjectionIdentities = state.pages.flatMap(
        // bind only projections in acknowledged closed pages
        (page) => page.projectionIdentities,
      ).sort();
      const gapHashes = state.gaps.map(
        // bind every durable permanent gap
        (gap) => gap.gapSha256,
      ).sort();

      // require final closure to match the live append-only state exactly
      if (manifest.dueKey !== state.dueKey || manifest.generation !== state.generation ||
        !canonicalJsonBytes(manifest.pages).equals(canonicalJsonBytes(projectedPages)) ||
        manifest.acknowledgementRootSha256 !==
          sha256(canonicalJsonBytes(acknowledgementHashes)) ||
        manifest.acknowledgedProjectionCount !== acknowledgedProjectionIdentities.length ||
        manifest.acknowledgedProjectionRootSha256 !==
          sha256(canonicalJsonBytes(acknowledgedProjectionIdentities)) ||
        manifest.evidenceGapCount !== gapHashes.length ||
        manifest.evidenceGapRootSha256 !== sha256(canonicalJsonBytes(gapHashes))) {
        throw new TypeError("finalized manifest state binding is invalid");
      }
    }

    return state;
  }

  // validate one immutable final page manifest
  function validateCycleFinalManifest(manifest) {
    const sealed = manifest?.contractVersion === ADJUSTMENT_CYCLE_SEALED_FINAL_MANIFEST_CONTRACT_VERSION;
    requireExactKeys(manifest, [
      ...(sealed ? ["inputSeal"] : []),
      "contractVersion",
      "dueKey",
      "generation",
      "finalizedAt",
      "pageCount",
      "totalPayloadBytes",
      "pages",
      "pageRootSha256",
      "acknowledgementRootSha256",
      "acknowledgedProjectionCount",
      "acknowledgedProjectionRootSha256",
      "evidenceGapCount",
      "evidenceGapRootSha256",
    ], "cycle final manifest");

    // require the exact immutable final contract
    if (!sealed && manifest.contractVersion !== ADJUSTMENT_CYCLE_FINAL_MANIFEST_CONTRACT_VERSION) {
      throw new TypeError("cycle final manifest contract is invalid");
    }
    requireDueKey(manifest.dueKey);
    requireUint64(manifest.generation, "generation");
    requireInstant(manifest.finalizedAt, "finalizedAt");
    requireSha256(manifest.pageRootSha256, "pageRootSha256");
    requireSha256(manifest.acknowledgementRootSha256, "acknowledgementRootSha256");
    requireSha256(manifest.acknowledgedProjectionRootSha256, "acknowledgedProjectionRootSha256");
    requireSha256(manifest.evidenceGapRootSha256, "evidenceGapRootSha256");

    // require exact bounded counts
    if (!Array.isArray(manifest.pages) || manifest.pageCount !== manifest.pages.length ||
      manifest.pageCount < 1 || manifest.pageCount > ADJUSTMENT_CYCLE_MAXIMUM_PAGES ||
      !Number.isSafeInteger(manifest.totalPayloadBytes) || manifest.totalPayloadBytes < 1 ||
      manifest.totalPayloadBytes > ADJUSTMENT_CYCLE_MAXIMUM_PAYLOAD_BYTES ||
      !Number.isSafeInteger(manifest.acknowledgedProjectionCount) ||
      manifest.acknowledgedProjectionCount < 0 ||
      !Number.isSafeInteger(manifest.evidenceGapCount) || manifest.evidenceGapCount < 0) {
      throw new TypeError("cycle final manifest bounds are invalid");
    }

    // require the exact ordered page root
    if (sha256(canonicalJsonBytes(manifest.pages)) !== manifest.pageRootSha256) {
      throw new TypeError("cycle final page root is invalid");
    }

    let expectedOffset = 0;
    let predecessorPageSha256 = null;

    // validate exact page ordering, offsets and predecessor links
    for (let index = 0; index < manifest.pages.length; index += 1) {
      const page = manifest.pages[index];
      requireExactKeys(page, [
        "pageIndex",
        "pageSha256",
        "predecessorPageSha256",
        "payloadSha256",
        "payloadOffset",
        "payloadLength",
      ], "cycle final page");
      requireSha256(page.pageSha256, "final pageSha256");
      requireSha256(page.payloadSha256, "final payloadSha256");

      // reject gaps, overlap and predecessor substitution
      if (page.pageIndex !== index || page.predecessorPageSha256 !== predecessorPageSha256 ||
        page.payloadOffset !== expectedOffset || !Number.isInteger(page.payloadLength) ||
        page.payloadLength < 1 || page.payloadLength > ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES) {
        throw new TypeError("cycle final page is invalid");
      }
      predecessorPageSha256 = page.pageSha256;
      expectedOffset += page.payloadLength;
    }

    // bind the exact total to the ordered ranges
    if (expectedOffset !== manifest.totalPayloadBytes) {
      throw new TypeError("cycle final payload total is invalid");
    }
    // recompute every closure cross-link instead of trusting a declaration
    if (sealed) {
      validateCycleInputSeal(manifest.inputSeal);
      const seal = manifest.inputSeal;
      if (seal.dueKey !== manifest.dueKey || seal.generation !== manifest.generation ||
        seal.pageRootSha256 !== manifest.pageRootSha256 ||
        seal.acknowledgementRootSha256 !== manifest.acknowledgementRootSha256 ||
        seal.acknowledgedProjectionCount !== manifest.acknowledgedProjectionCount ||
        seal.acknowledgedProjectionRootSha256 !== manifest.acknowledgedProjectionRootSha256 ||
        seal.evidenceGapRootSha256 !== manifest.evidenceGapRootSha256 ||
        Date.parse(seal.sealedAt) > Date.parse(manifest.finalizedAt)) {
        throw new TypeError("cycle input seal does not match final manifest");
      }
    }
    return manifest;
  }

  // validate one bounded combined-input declaration without granting qualification
  function validateCycleInputSeal(seal) {
    requireExactKeys(seal, [
      "acknowledgedProjectionCount", "acknowledgedProjectionRootSha256",
      "acknowledgementRootSha256", "contractVersion", "dueKey", "evidenceGapRootSha256",
      "generation", "pageRootSha256", "requiredInputs", "sealedAt",
    ], "cycle input seal");
    if (seal.contractVersion !== "adjustment-cycle-input-seal/v1") {
      throw new TypeError("cycle input seal contract is invalid");
    }
    requireDueKey(seal.dueKey);
    requireUint64(seal.generation, "seal generation");
    requireInstant(seal.sealedAt, "sealedAt");
    // validate the exact page and metadata root identities
    for (const field of ["acknowledgedProjectionRootSha256", "acknowledgementRootSha256",
      "evidenceGapRootSha256", "pageRootSha256"]) {
      requireSha256(seal[field], field);
    }
    if (!Number.isSafeInteger(seal.acknowledgedProjectionCount) ||
      seal.acknowledgedProjectionCount < 1 ||
      seal.acknowledgedProjectionCount > ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS) {
      throw new TypeError("cycle input seal count is invalid");
    }
    const kinds = ["actual_best_match", "native_source", "rain_gate_input",
      "serving_evidence", "shadow_body", "shadow_metadata", "target_revision"];
    requireExactKeys(seal.requiredInputs, kinds, "required cycle inputs");
    // require every mandatory class to be explicit even when its population is empty
    for (const kind of kinds) {
      const entry = seal.requiredInputs[kind];
      requireExactKeys(entry, ["count", "rootSha256"], "required input root");
      requireSha256(entry.rootSha256, "required input rootSha256");
      if (!Number.isSafeInteger(entry.count) || entry.count < 0 || entry.count > 8_192 ||
        (entry.count === 0 && entry.rootSha256 !== sha256(canonicalJsonBytes([])))) {
        throw new TypeError("required input population is invalid");
      }
    }
    if (seal.requiredInputs.serving_evidence.count < 1 ||
      seal.requiredInputs.shadow_body.count !== seal.requiredInputs.shadow_metadata.count ||
      canonicalJsonBytes(seal).length > 2_048) {
      throw new TypeError("cycle input seal bounds are invalid");
    }
    return seal;
  }

  // encode one cold capsule member for one finalized cycle
  function encodeCycleCapsule(input) {
    requireExactKeys(input, ["manifest", "manifestSha256", "pagePayloads"], "cycle capsule input");
    const manifest = validateCycleFinalManifest(input.manifest);
    requireSha256(input.manifestSha256, "manifestSha256");
    const manifestBytes = canonicalJsonBytes(manifest);

    // bind the exact immutable final manifest
    if (sha256(manifestBytes) !== input.manifestSha256) {
      throw new TypeError("cycle capsule manifest hash is invalid");
    }

    // require exactly one payload per finalized page
    if (!Array.isArray(input.pagePayloads) ||
      input.pagePayloads.length !== manifest.pageCount) {
      throw new TypeError("cycle capsule payload count is invalid");
    }

    const payloads = input.pagePayloads.map(
      // copy and verify every ordered page payload
      (payload, index) => {
        const bytes = requireBuffer(payload, "cycle capsule page payload");
        const page = manifest.pages[index];

        // reject substituted or resized page bytes
        if (bytes.length !== page.payloadLength || sha256(bytes) !== page.payloadSha256) {
          throw new TypeError("cycle capsule page payload is invalid");
        }
        return Buffer.from(bytes);
      },
    );
    const header = Buffer.alloc(CAPSULE_HEADER_BYTES);
    CAPSULE_MAGIC.copy(header, 0);
    header.writeUInt32BE(manifestBytes.length, 8);
    header.writeUInt32BE(manifest.pageCount, 12);
    return Buffer.concat([header, manifestBytes, ...payloads]);
  }

  // validate one single-capsule finalized-cycle payload
  function validateCycleCapsule(bytes) {
    const payload = requireBuffer(bytes, "cycle capsule bytes");

    // reject a truncated or wrong-magic capsule
    if (payload.length < CAPSULE_HEADER_BYTES ||
      !payload.subarray(0, 8).equals(CAPSULE_MAGIC)) {
      throw new TypeError("cycle capsule header is invalid");
    }
    const manifestLength = payload.readUInt32BE(8);
    const pageCount = payload.readUInt32BE(12);
    const manifestEnd = CAPSULE_HEADER_BYTES + manifestLength;

    // reject unsafe manifest bounds before parsing
    if (manifestLength === 0 || manifestEnd > payload.length) {
      throw new TypeError("cycle capsule manifest length is invalid");
    }

    const manifestBytes = payload.subarray(CAPSULE_HEADER_BYTES, manifestEnd);
    let manifest;

    try {
      manifest = JSON.parse(manifestBytes.toString("utf8"));
    } catch {
      throw new TypeError("cycle capsule manifest JSON is invalid");
    }

    // require exact canonical manifest bytes
    if (!canonicalJsonBytes(manifest).equals(manifestBytes)) {
      throw new TypeError("cycle capsule manifest is not canonical");
    }
    validateCycleFinalManifest(manifest);

    // bind the redundant capsule page count
    if (pageCount !== manifest.pageCount) {
      throw new TypeError("cycle capsule page count is invalid");
    }

    let offset = manifestEnd;

    // verify every page payload in final-manifest order
    for (const page of manifest.pages) {
      const end = offset + page.payloadLength;

      // reject short page bytes before hashing
      if (end > payload.length) {
        throw new TypeError("cycle capsule is truncated");
      }

      // reject a substituted page payload
      if (sha256(payload.subarray(offset, end)) !== page.payloadSha256) {
        throw new TypeError("cycle capsule page hash is invalid");
      }
      offset = end;
    }

    // reject hidden trailing rows or a second capsule
    if (offset !== payload.length) {
      throw new TypeError("cycle capsule has trailing bytes");
    }
    return {
      contractVersion: ADJUSTMENT_CYCLE_CAPSULE_CONTRACT_VERSION,
      dueKey: manifest.dueKey,
      generation: manifest.generation,
      manifest,
      manifestSha256: sha256(manifestBytes),
      pageCount,
      totalPayloadBytes: manifest.totalPayloadBytes,
    };
  }

  // validate one transport page envelope for edge integration
  function validateCyclePage(input) {
    requireExactKeys(input, ["header", "pageSha256", "payload", "projections"], "cycle page envelope");
    const header = input.header;
    requireExactKeys(header, [
      "contractVersion",
      "dueKey",
      "generation",
      "pageIndex",
      "predecessorPageSha256",
      "payloadSha256",
      "payloadOffset",
    ], "cycle page header");

    // require the online header contract without final root or count fields
    if (header.contractVersion !== ADJUSTMENT_CYCLE_PAGE_CONTRACT_VERSION) {
      throw new TypeError("cycle page contract is invalid");
    }
    requireDueKey(header.dueKey);
    requireUint64(header.generation, "generation");
    requireSha256(header.payloadSha256, "payloadSha256");

    // validate nullable predecessor, index and offset bounds
    if (header.predecessorPageSha256 !== null) {
      requireSha256(header.predecessorPageSha256, "predecessorPageSha256");
    }

    if (!Number.isInteger(header.pageIndex) || header.pageIndex < 0 ||
      header.pageIndex >= ADJUSTMENT_CYCLE_MAXIMUM_PAGES ||
      !Number.isSafeInteger(header.payloadOffset) || header.payloadOffset < 0) {
      throw new TypeError("cycle page header bounds are invalid");
    }

    // require genesis or predecessor-bound header consistency
    if ((header.pageIndex === 0 &&
        (header.predecessorPageSha256 !== null || header.payloadOffset !== 0)) ||
      (header.pageIndex > 0 && header.predecessorPageSha256 === null)) {
      throw new TypeError("cycle page predecessor is invalid");
    }

    const payload = requireBuffer(input.payload, "page payload");

    // require exact payload and complete-page hashes
    if (payload.length < 1 || payload.length > ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES ||
      sha256(payload) !== header.payloadSha256 ||
      hashPage(header, payload) !== input.pageSha256) {
      throw new TypeError("cycle page bytes are invalid");
    }
    requireSha256(input.pageSha256, "pageSha256");
    const projections = normalizeProjections(input.projections);
    return { header: { ...header }, pageSha256: input.pageSha256, payload, projections };
  }

  // normalize one projection identity and channel
  function normalizeProjections(projections) {
    // require a bounded projection list
    if (!Array.isArray(projections) || projections.length > ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS) {
      throw new TypeError("page projections are invalid");
    }

    const normalized = projections.map(
      // validate one immutable projection reference
      (projection) => {
        requireExactKeys(projection, ["channel", "identitySha256"], "page projection");
        requireSha256(projection.identitySha256, "projection identitySha256");

        // allow only reserved scheduler or ordinary organic projections
        if (projection.channel !== "scheduler_request" && projection.channel !== "organic") {
          throw new TypeError("projection channel is invalid");
        }
        return { ...projection };
      },
    ).sort(
      // order exact projection identities
      (left, right) => left.identitySha256.localeCompare(right.identitySha256, "en"),
    );

    // reject duplicate projection identities
    if (normalized.some((projection, index) =>
      index > 0 && projection.identitySha256 === normalized[index - 1].identitySha256)) {
      throw new TypeError("projection identity is duplicated");
    }
    return normalized;
  }

  // count projection channels without pooling them
  function countProjections(projections) {
    return {
      organic: projections.filter(
        // select ordinary projections
        (projection) => projection.channel === "organic",
      ).length,
      scheduler: projections.filter(
        // select scheduler reservations
        (projection) => projection.channel === "scheduler_request",
      ).length,
    };
  }

  // count one retained page channel
  function countPageChannels(pages, channel) {
    return pages.reduce(
      // accumulate exact page channel entries
      (total, page) => total + page.projectionChannels.filter(
        // match one requested channel
        (value) => value === channel,
      ).length,
      0,
    );
  }

  // validate one page metadata record
  function normalizePageMetadata(value, state, index) {
    requireExactKeys(value, [
      "pageIndex",
      "pageSha256",
      "predecessorPageSha256",
      "payloadSha256",
      "payloadOffset",
      "payloadLength",
      "projectionIdentities",
      "projectionChannels",
    ], "durable page");
    requireSha256(value.pageSha256, "page pageSha256");
    requireSha256(value.payloadSha256, "page payloadSha256");

    // require exact consecutive index, predecessor and offset
    if (value.pageIndex !== index ||
      value.predecessorPageSha256 !== (index === 0 ? null : state.pages[index - 1].pageSha256) ||
      value.payloadOffset !== (index === 0
        ? 0
        : state.pages[index - 1].payloadOffset + state.pages[index - 1].payloadLength) ||
      !Number.isInteger(value.payloadLength) || value.payloadLength < 1 ||
      value.payloadLength > ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES ||
      !Array.isArray(value.projectionIdentities) || !Array.isArray(value.projectionChannels) ||
      value.projectionIdentities.length !== value.projectionChannels.length) {
      throw new TypeError("page metadata is invalid");
    }

    // validate every retained projection identity
    for (const identity of value.projectionIdentities) {
      requireSha256(identity, "page projection identity");
    }

    // validate every retained projection channel
    for (const channel of value.projectionChannels) {
      // reject unknown projection channels
      if (channel !== "organic" && channel !== "scheduler_request") {
        throw new TypeError("page projection channel is invalid");
      }
    }
    return { ...value };
  }

  // normalize one durable acknowledgement
  function normalizeDurableAcknowledgement(value, state, pages, index) {
    requireExactKeys(value, [
      "contractVersion",
      "dueKey",
      "generation",
      "pageIndex",
      "pageSha256",
      "acknowledgedAt",
      "acknowledgementSha256",
    ], "durable acknowledgement");

    // require one exact ordered acknowledgement
    if (value.contractVersion !== ADJUSTMENT_CYCLE_ACK_CONTRACT_VERSION ||
      value.dueKey !== state.dueKey || value.generation !== state.generation ||
      value.pageIndex !== index || value.pageSha256 !== pages[index]?.pageSha256) {
      throw new TypeError("durable acknowledgement is invalid");
    }
    requireInstant(value.acknowledgedAt, "acknowledgedAt");
    requireSha256(value.acknowledgementSha256, "acknowledgementSha256");
    const unhashed = { ...value };
    delete unhashed.acknowledgementSha256;

    // bind the exact acknowledgement bytes
    if (sha256(canonicalJsonBytes(unhashed)) !== value.acknowledgementSha256) {
      throw new TypeError("acknowledgement hash is invalid");
    }
    return { ...value };
  }

  // validate one retained evidence gap
  function validateEvidenceGap(value, state) {
    requireExactKeys(value, [
      "contractVersion",
      "dueKey",
      "generation",
      "projectionIdentitySha256",
      "reason",
      "occurredAt",
      "qualificationDisposition",
      "gapSha256",
    ], "evidence gap");

    // require the exact permanent-unqualification semantics
    if (value.contractVersion !== ADJUSTMENT_EVIDENCE_GAP_CONTRACT_VERSION ||
      value.dueKey !== state.dueKey || value.generation !== state.generation ||
      value.qualificationDisposition !== "forever_unqualified" ||
      !GAP_REASONS.has(value.reason)) {
      throw new TypeError("evidence gap is invalid");
    }
    requireSha256(value.projectionIdentitySha256, "gap projectionIdentitySha256");
    requireInstant(value.occurredAt, "gap occurredAt");
    requireSha256(value.gapSha256, "gapSha256");
    const unhashed = { ...value };
    delete unhashed.gapSha256;

    // bind the exact gap bytes
    if (sha256(canonicalJsonBytes(unhashed)) !== value.gapSha256) {
      throw new TypeError("evidence gap hash is invalid");
    }
    return { ...value };
  }

  // compare two arrays as canonical prefixes
  function isPrefix(prefix, complete) {
    // reject a longer alleged prefix
    if (prefix.length > complete.length) {
      return false;
    }
    return prefix.every(
      // compare each canonical element byte for byte
      (value, index) => canonicalJsonBytes(value).equals(canonicalJsonBytes(complete[index])),
    );
  }

  // hash the canonical header framing and exact payload
  function hashPage(header, payload) {
    return createHash("sha256").update(canonicalJsonBytes(header)).update(payload).digest("hex");
  }

  // require an exact fsync durability proof
  function requireDurabilityProof(value, hashKey, expectedSha256) {
    requireExactKeys(value, ["fsynced", hashKey], "durability proof");

    // reject optimistic or mismatched persistence claims
    if (value.fsynced !== true || value[hashKey] !== expectedSha256) {
      throw pageError("durability_proof_refused");
    }
  }

  // require one persistence boundary
  function requireDurabilityPort(value, label) {
    // reject missing persistence instead of mocking success
    if (typeof value !== "function") {
      throw new TypeError(`${label} is required`);
    }
  }

  // classify one page failure into a durable gap category
  function classifyPageFailure(error) {
    // preserve only closed gap reason values
    if (GAP_REASONS.has(error?.reason)) {
      return error.reason;
    }
    return error?.code === "resource_refused" ? "capacity_refused" : "archive_refused";
  }

  // create one closed page refusal
  function pageError(reason) {
    const error = new Error(reason);
    error.code = "adjustment_page_refused";
    error.reason = reason;
    return error;
  }

  // encode recursively sorted canonical JSON
  function canonicalJsonBytes(value) {
    return Buffer.from(`${JSON.stringify(canonicalValue(value))}\n`, "utf8");
  }

  // recursively normalize canonical JSON values
  function canonicalValue(value) {
    // retain JSON scalar values
    if (value === null || typeof value === "string" || typeof value === "boolean") {
      return value;
    }

    // retain only safe finite nonnegative-zero numbers
    if (typeof value === "number") {
      // reject nonfinite and negative-zero encodings
      if (!Number.isFinite(value) || Object.is(value, -0)) {
        throw new TypeError("canonical JSON number is invalid");
      }
      return value;
    }

    // preserve semantic array order
    if (Array.isArray(value)) {
      return value.map(
        // normalize one array value
        (entry) => canonicalValue(entry),
      );
    }

    requirePlainObject(value, "canonical JSON object");
    return Object.fromEntries(Object.keys(value).sort().map(
      // sort and normalize every object field
      (key) => [key, canonicalValue(value[key])],
    ));
  }

  // calculate one lowercase SHA-256 digest
  function sha256(value) {
    return createHash("sha256").update(value).digest("hex");
  }

  // require the exact scheduled capture due-key grammar
  function requireDueKey(value) {
    // reject ambiguous or non-cycle due keys
    if (typeof value !== "string" || !DUE_KEY_PATTERN.test(value)) {
      throw new TypeError("dueKey is invalid");
    }
  }

  // require one canonical local date
  function requireLocalDate(value) {
    // reject noncanonical dates
    if (typeof value !== "string" || !LOCAL_DATE_PATTERN.test(value)) {
      throw new TypeError("localDate is invalid");
    }
  }

  // require one canonical UTC millisecond instant
  function requireInstant(value, label) {
    // reject noncanonical or impossible instants
    if (typeof value !== "string" || !INSTANT_PATTERN.test(value) ||
      new Date(value).toISOString() !== value) {
      throw new TypeError(`${label} is invalid`);
    }
  }

  // require one decimal unsigned 64-bit generation
  function requireUint64(value, label) {
    // reject noncanonical or overflowing decimal strings
    if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/u.test(value) ||
      BigInt(value) > UINT64_MAXIMUM) {
      throw new TypeError(`${label} is invalid`);
    }
  }

  // require one lowercase SHA-256 value
  function requireSha256(value, label) {
    // reject noncanonical hashes
    if (typeof value !== "string" || !SHA256_PATTERN.test(value)) {
      throw new TypeError(`${label} is invalid`);
    }
  }

  // require one immutable byte buffer
  function requireBuffer(value, label) {
    // reject implicit byte coercion
    if (!Buffer.isBuffer(value)) {
      throw new TypeError(`${label} must be a Buffer`);
    }
    return value;
  }

  // require one plain object
  function requirePlainObject(value, label) {
    // reject null, arrays and custom prototypes
    if (value === null || Array.isArray(value) || typeof value !== "object" ||
      Object.getPrototypeOf(value) !== Object.prototype) {
      throw new TypeError(`${label} must be a plain object`);
    }
  }

  // require one exact closed key set
  function requireExactKeys(value, keys, label) {
    requirePlainObject(value, label);
    const actual = Object.keys(value).sort();
    const expected = [...keys].sort();

    // reject unknown or missing fields
    if (actual.length !== expected.length ||
      actual.some((key, index) => key !== expected[index])) {
      throw new TypeError(`${label} has invalid keys`);
    }
  }


  return Object.freeze({
    ADJUSTMENT_CYCLE_PAGE_CONTRACT_VERSION,
    ADJUSTMENT_CYCLE_ACK_CONTRACT_VERSION,
    ADJUSTMENT_CYCLE_FINAL_MANIFEST_CONTRACT_VERSION,
    ADJUSTMENT_CYCLE_SEALED_FINAL_MANIFEST_CONTRACT_VERSION,
    ADJUSTMENT_CYCLE_CAPSULE_CONTRACT_VERSION,
    ADJUSTMENT_EVIDENCE_GAP_CONTRACT_VERSION,
    ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES,
    ADJUSTMENT_CYCLE_MAXIMUM_PAGES,
    ADJUSTMENT_CYCLE_MAXIMUM_PAYLOAD_BYTES,
    ADJUSTMENT_DAILY_MAXIMUM_PAGES,
    ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS,
    ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS,
    createCyclePageState,
    appendCyclePage,
    ackCyclePage,
    finalizeCyclePages,
    reconcileCyclePages,
    recordEvidenceGap,
    appendCyclePageFailOpen,
    validateCyclePageState,
    validateCycleFinalManifest,
    validateCycleInputSeal,
    encodeCycleCapsule,
    validateCycleCapsule,
    validateCyclePage,
  });
})();

// export the fixed cycle protocol surface for runtime and research callers
export const {
  ADJUSTMENT_CYCLE_PAGE_CONTRACT_VERSION,
  ADJUSTMENT_CYCLE_ACK_CONTRACT_VERSION,
  ADJUSTMENT_CYCLE_FINAL_MANIFEST_CONTRACT_VERSION,
  ADJUSTMENT_CYCLE_SEALED_FINAL_MANIFEST_CONTRACT_VERSION,
  ADJUSTMENT_CYCLE_CAPSULE_CONTRACT_VERSION,
  ADJUSTMENT_EVIDENCE_GAP_CONTRACT_VERSION,
  ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES,
  ADJUSTMENT_CYCLE_MAXIMUM_PAGES,
  ADJUSTMENT_CYCLE_MAXIMUM_PAYLOAD_BYTES,
  ADJUSTMENT_DAILY_MAXIMUM_PAGES,
  ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS,
  ADJUSTMENT_DAILY_RESERVED_SCHEDULER_PROJECTIONS,
  createCyclePageState,
  appendCyclePage,
  ackCyclePage,
  finalizeCyclePages,
  reconcileCyclePages,
  recordEvidenceGap,
  appendCyclePageFailOpen,
  validateCyclePageState,
  validateCycleFinalManifest,
  validateCycleInputSeal,
  encodeCycleCapsule,
  validateCycleCapsule,
  validateCyclePage,
} = adjustmentCyclePageProtocol;

export const ADJUSTMENT_EVIDENCE_OBJECT_CONTRACT_VERSION =
  "forecast-adjustment-evidence-object/v1";
export const ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION =
  "forecast-adjustment-edge-receipt/v1";
export const ADJUSTMENT_EVIDENCE_SNAPSHOT_CONTRACT_VERSION =
  "forecast-adjustment-evidence-snapshot/v1";
export const ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION =
  "forecast-adjustment-evidence-object/v2";
export const ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION =
  "forecast-adjustment-edge-receipt/v2";
export const ADJUSTMENT_EVIDENCE_CHANNEL_CONTRACT_VERSION =
  "issuance-channel-assertion/v2";
export const ADJUSTMENT_EVIDENCE_SCHEDULER_STATE_CONTRACT_VERSION =
  "adjustment-evidence-scheduler-state/v1";
export const ADJUSTMENT_EVIDENCE_DEFAULT_ROOT =
  "/var/lib/weather/xweather/adjustment-evidence";
const ADJUSTMENT_EVIDENCE_PROTECTED_FREE_BYTES = 2_030_043_136;
const ADJUSTMENT_EVIDENCE_NEXT_CAPTURE_RESERVATION_BYTES = 4_112_384;
export const ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES =
  ADJUSTMENT_EVIDENCE_PROTECTED_FREE_BYTES +
  ADJUSTMENT_EVIDENCE_NEXT_CAPTURE_RESERVATION_BYTES;
export const ADJUSTMENT_EVIDENCE_CAPTURE_START = "2026-10-08T00:00:00.000Z";
export const ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION =
  "adjustment-archive-transfer/v1";
export const ADJUSTMENT_MAINTENANCE_ANCHOR_CONTRACT_VERSION =
  "adjustment-maintenance-anchor/v2";
export const ADJUSTMENT_FUTURE_ONLY_INPUT_SEAL_CONTRACT_VERSION =
  "adjustment-future-only-input-seal/v2";
export const ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION =
  "adjustment-maintenance-anchor/v3";
export const ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_V3_CONTRACT_VERSION =
  "adjustment-maintenance-finalization-proof/v3";
export const ADJUSTMENT_DEVELOPMENT_CUSTODY_ANCHOR_CONTRACT_VERSION =
  "adjustment-development-custody-anchor/v1";
export const ADJUSTMENT_RAIN_CONTROL_CUSTODY_ANCHOR_CONTRACT_VERSION =
  "adjustment-rain-control-custody-anchor/v1";
export const ADJUSTMENT_UNSUPPORTED_TERMINAL_PROOF_CONTRACT_VERSION =
  "adjustment-maintenance-unsupported-terminal-proof/v1";
export const ADJUSTMENT_SHADOW_METADATA_CUSTODY_PREPARATION_CONTRACT_VERSION =
  "adjustment-shadow-metadata-custody-preparation/v1";
export const ADJUSTMENT_SHADOW_METADATA_CUSTODY_STATUS_CONTRACT_VERSION =
  "adjustment-shadow-metadata-custody-status/v1";
export const ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES = 16 * 1_024;
export const ADJUSTMENT_REVISION_COMMIT_RECEIPT_CONTRACT_VERSION =
  "adjustment-revision-commit-receipt/v1";
export const ADJUSTMENT_REVISION_CATALOG_FRONTIER_CONTRACT_VERSION =
  "adjustment-revision-catalog-frontier/v1";
export const ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_CONTRACT_VERSION =
  "adjustment-maintenance-finalization-proof/v1";
export const ADJUSTMENT_REVISION_CAPTURE_EPOCH_CONTRACT_VERSION =
  "adjustment-revision-capture-epoch-witness/v1";
export const ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH =
  "/opt/weather/current/deploy/state/adjustment-capture-epoch.json";
export const ADJUSTMENT_REVISION_CAPTURE_EPOCH_SNAPSHOT_PATH =
  "/opt/weather/current/deploy/state/adjustment-capture-epoch-snapshot.json";
const ADJUSTMENT_REVISION_CAPTURE_EPOCH_MAXIMUM_BYTES = 16 * 1024;
const ADJUSTMENT_REVISION_FRONTIER_MIGRATION_HISTORY_SHA256 =
  "c683c4f937c7f02b00f6ab49f75268eead81d2a221a8a9a23e38f9e4802a11b0";
const ADJUSTMENT_ROLLING_REGISTRATION_MIGRATION_HISTORY_SHA256 =
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const ADJUSTMENT_CAPTURE_EPOCH_DATABASE_LEDGERS = Object.freeze([Object.freeze({
  checksum: "56027b4f2cb2c3c83e746f14dac753b75d573753934898fa7a52f78faa56499c",
  count: 20,
  historySha256: ADJUSTMENT_REVISION_FRONTIER_MIGRATION_HISTORY_SHA256,
  name: "0020_adjustment_revision_frontier.sql",
}), Object.freeze({
  checksum: "ca29db99377001fca2e2e4268fd80d1cbe7b876f71f2d5ba04e575712cb9f13b",
  count: 21,
  historySha256: ADJUSTMENT_ROLLING_REGISTRATION_MIGRATION_HISTORY_SHA256,
  name: "0021_adjustment_rolling_registration.sql",
})]);

const OBJECT_MAXIMUM_BYTES = 512 * 1_024;
const COMPRESSED_OBJECT_MAXIMUM_BYTES = 16 * 1_024;
const RECEIPT_MAXIMUM_BYTES = 16 * 1_024;
const MAXIMUM_COMPRESSION_RATIO = 64;
const MAXIMUM_ROWS = 240;
const MAXIMUM_V2_ROWS = 241;
const COMPRESSION_CACHE_MAXIMUM_ENTRIES = 8;
const MAXIMUM_IDENTITIES = 8_192;
const MAXIMUM_CHANNEL_ASSERTIONS = 256;
const SCHEDULER_HISTORY_LIMIT = 4 * 365;
const SCHEDULER_CATCHUP_MILLISECONDS = 12 * 60 * 60 * 1_000;
const SCHEDULER_POLL_MILLISECONDS = 30_000;
const ARCHIVE_LOCK_LEASE_MILLISECONDS = 5 * 60 * 1_000;
const ARCHIVE_LOCK_ATTEMPTS = 200;
const ARCHIVE_LOCK_RETRY_MILLISECONDS = 10;
const ARCHIVE_TRANSFER_MAXIMUM_BYTES = 1_024 * 1_024;
const LEDGER_ALLOCATION_MAXIMUM_BYTES = 64 * 1_024 * 1_024;
const FREE_INODE_FLOOR = 32_768;
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const ADJUSTMENT_ROLLING_SCHEDULE_SHA256 =
  "7c17f5d1a8e8249cd0aa4820638169e51f6edb3433017f50ab4c959e44c62f1f";
const OBJECT_FILENAME_PATTERN = /^sha256-([a-f0-9]{64})\.json\.gz$/u;
const RECEIPT_FILENAME_PATTERN = /^sha256-([a-f0-9]{64})\.json$/u;
const CHANNEL_FILENAME_PATTERN = /^sha256-([a-f0-9]{64})\.json$/u;
const SCHEDULE_HOURS = [0, 6, 12, 18];
const ISSUANCE_CHANNELS = ["legacy_unattributed", "public_get", "scheduler_request"];
const WINDOWS = ["days=1", "days=5", "days=10", "overnight"];
const WIND_DECISION_STATES = ["active", "disabled", "not_applicable"];
const SOURCE_DECISION_STATES = ["active", "disabled", "raw_fallback"];
const RAW_METRICS = [
  "precipitationMm",
  "temperatureC",
  "windGustMps",
  "windSpeedMps",
];
const ADJUSTED_METRICS = [
  "apparentTemperatureC",
  "relativeHumidityPercent",
  "temperatureC",
  "windDirectionDegrees",
  "windGustMps",
  "windSpeedMps",
];

// derive one authoritative frontier from exact canonical database receipts
export function deriveAdjustmentRevisionCatalogFrontier(receiptsValue) {
  // require a finite complete receipt population from genesis
  if (!Array.isArray(receiptsValue) || receiptsValue.length < 1) {
    throw new TypeError("adjustment revision receipt population is invalid");
  }
  const genesis = sha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
  let predecessor = genesis;
  let priorOrdinal = 0n;
  const receiptHashes = [];

  // recompute every server receipt and predecessor link in ordinal order
  for (const receipt of receiptsValue) {
    validateRevisionCommitReceipt(receipt);
    const ordinal = BigInt(receipt.archiveCommitOrdinal);
    const computedReceiptSha256 = sha256(Buffer.from([
      ADJUSTMENT_REVISION_COMMIT_RECEIPT_CONTRACT_VERSION,
      receipt.archiveCommitOrdinal,
      receipt.archiveCommittedAt,
      receipt.projectionKind,
      receipt.projectionIdentitySha256,
      receipt.projectionSha256,
      receipt.stageReceiptSha256,
      predecessor,
    ].join("\n")));
    const computedFrontierSha256 = sha256(Buffer.from([
      "adjustment-revision-frontier/v1",
      predecessor,
      receipt.archiveCommitOrdinal,
      computedReceiptSha256,
    ].join("\n")));

    // reject reordered, substituted or caller-hashed receipt members
    if (ordinal <= priorOrdinal || receipt.predecessorFrontierSha256 !== predecessor ||
      receipt.receiptSha256 !== computedReceiptSha256 ||
      receipt.frontierSha256 !== computedFrontierSha256) {
      throw new Error("adjustment revision receipt chain is invalid");
    }
    predecessor = computedFrontierSha256;
    priorOrdinal = ordinal;
    receiptHashes.push(computedReceiptSha256);
  }
  return Object.freeze({
    archiveCommitOrdinal: priorOrdinal.toString(),
    contractVersion: ADJUSTMENT_REVISION_CATALOG_FRONTIER_CONTRACT_VERSION,
    frontierCount: receiptsValue.length,
    frontierRootSha256: predecessor,
    receiptRootSha256: sha256(Buffer.from(`${canonicalJson(receiptHashes)}\n`)),
  });
}

// verify one complete bounded cold receipt chain against its frozen hot snapshot
export function verifyAdjustmentRevisionColdCatalog(input) {
  requireExactKeys(input, ["receipts", "servingSnapshot"],
    "adjustment revision cold catalog input");
  const frontier = deriveAdjustmentRevisionCatalogFrontier(input.receipts);
  const snapshot = validateAdjustmentRevisionServingSnapshot(input.servingSnapshot);
  const receiptsBySha256 = new Map(input.receipts.map(
    // index only immutable database receipt identities
    (receipt) => [receipt.receiptSha256, receipt],
  ));
  // bind the exact database watermark and every current pointer into cold members
  if (snapshot.archiveCommitOrdinal !== frontier.archiveCommitOrdinal ||
    snapshot.frontierSha256 !== frontier.frontierRootSha256 ||
    snapshot.entries.some((entry) =>
      canonicalJson(receiptsBySha256.get(entry.receipt.receiptSha256)) !==
        canonicalJson(entry.receipt))) {
    throw new Error("adjustment revision cold catalog differs from serving snapshot");
  }
  const kindCounts = Object.fromEntries([
    "actual_best_match", "native_source", "rain_gate_input", "target_revision",
  ].map((kind) => [kind, snapshot.entries.filter(
    // count only qualified current pointers, never terminal ordinal gaps
    (entry) => entry.receipt.projectionKind === kind,
  ).length]));
  // require all four genuine revision producer classes before semantic readiness
  if (Object.values(kindCounts).some((count) => count < 1)) {
    throw new Error("adjustment revision cold catalog producer population is incomplete");
  }
  const catalog = {
    archiveCommitOrdinal: frontier.archiveCommitOrdinal,
    contractVersion: "adjustment-revision-cold-catalog/v1",
    frontierCount: frontier.frontierCount,
    frontierRootSha256: frontier.frontierRootSha256,
    kindCounts,
    receiptRootSha256: frontier.receiptRootSha256,
    servingSnapshotSha256: snapshot.snapshotSha256,
  };
  return Object.freeze({
    ...catalog,
    catalogRootSha256: sha256(canonicalJsonBytes(catalog)),
  });
}

// validate one root-authenticated future-only capture epoch witness
export function validateAdjustmentRevisionCaptureEpochWitness(value) {
  requireExactKeys(value, [
    "activationKind", "archiveCommitOrdinal", "catalogFrontierSha256",
    "contractVersion", "controlPlaneSha256", "controlPlaneVersion",
    "databaseMigrationHistorySha256", "epochAt", "servingSnapshotSha256",
    "sourceCommit", "sourceRelease", "sourceServerImageDigest",
    "sourceWebImageDigest", "witnessSha256",
  ], "adjustment revision capture epoch witness");
  // validate every fixed-width witness identity
  for (const field of [
    "catalogFrontierSha256", "controlPlaneSha256", "databaseMigrationHistorySha256",
    "servingSnapshotSha256", "witnessSha256",
  ]) {
    requireSha256(value[field], `adjustment revision capture epoch ${field}`);
  }
  requireInstant(value.epochAt, "adjustment revision capture epoch time");
  const releasePattern = /^\d{4}\.\d{2}\.\d{2}-[1-9][0-9]?$/u;
  const imageDigestPattern = /^sha256:[a-f0-9]{64}$/u;
  // bind the one-time witness to the exact inert-v14 pre-activation identity
  if (value.activationKind !== "inert_v14_pre_activation" ||
    value.archiveCommitOrdinal !== "0" ||
    value.catalogFrontierSha256 !== adjustmentRevisionGenesisFrontier() ||
    value.contractVersion !== ADJUSTMENT_REVISION_CAPTURE_EPOCH_CONTRACT_VERSION ||
    value.controlPlaneVersion !== "14" ||
    !ADJUSTMENT_CAPTURE_EPOCH_DATABASE_LEDGERS.some(
      // admit only one complete reviewed activation ledger
      (ledger) => value.databaseMigrationHistorySha256 === ledger.historySha256,
    ) ||
    typeof value.sourceCommit !== "string" || !/^[a-f0-9]{40}$/u.test(value.sourceCommit) ||
    typeof value.sourceRelease !== "string" || !releasePattern.test(value.sourceRelease) ||
    typeof value.sourceServerImageDigest !== "string" ||
      !imageDigestPattern.test(value.sourceServerImageDigest) ||
    typeof value.sourceWebImageDigest !== "string" ||
      !imageDigestPattern.test(value.sourceWebImageDigest)) {
    throw new TypeError("adjustment revision capture epoch witness is invalid");
  }
  const unsigned = {
    activationKind: value.activationKind,
    archiveCommitOrdinal: value.archiveCommitOrdinal,
    catalogFrontierSha256: value.catalogFrontierSha256,
    contractVersion: value.contractVersion,
    controlPlaneSha256: value.controlPlaneSha256,
    controlPlaneVersion: value.controlPlaneVersion,
    databaseMigrationHistorySha256: value.databaseMigrationHistorySha256,
    epochAt: value.epochAt,
    servingSnapshotSha256: value.servingSnapshotSha256,
    sourceCommit: value.sourceCommit,
    sourceRelease: value.sourceRelease,
    sourceServerImageDigest: value.sourceServerImageDigest,
    sourceWebImageDigest: value.sourceWebImageDigest,
  };
  // bind every unsigned canonical field without treating the hash as authority
  if (value.witnessSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_REVISION_CAPTURE_EPOCH_MAXIMUM_BYTES) {
    throw new Error("adjustment revision capture epoch identity differs");
  }
  return Object.freeze(value);
}

// create one future-only witness before the first v14 producer can start
export async function writeAdjustmentRevisionCaptureEpochWitness(input) {
  requireExactKeys(input, ["databaseEnvelope", "deployment", "path"],
    "adjustment revision capture epoch input");
  const path = resolve(input.path);
  const snapshotPath = join(dirname(path), "adjustment-capture-epoch-snapshot.json");
  const deployment = validateAdjustmentRevisionCaptureEpochDeployment(input.deployment);
  const database = validateAdjustmentRevisionCaptureEpochDatabaseEnvelope(
    input.databaseEnvelope,
  );
  const existing = await readAdjustmentRevisionCaptureEpochWitness({
    expectedGid: process.getegid(),
    expectedUid: process.geteuid(),
    path,
    optional: true,
  });
  const retainedSnapshot = await readAdjustmentRevisionCaptureEpochSnapshot({
    expectedGid: process.getegid(),
    expectedUid: process.geteuid(),
    optional: true,
    path: snapshotPath,
  });
  // preserve the first zero proof and server clock on an exact target retry
  if (existing !== null) {
    if (!adjustmentRevisionCaptureEpochDeploymentMatches(existing, deployment) ||
      retainedSnapshot === null ||
      retainedSnapshot.snapshotSha256 !== existing.servingSnapshotSha256 ||
      retainedSnapshot.cutoffAt !== existing.epochAt) {
      throw new Error("adjustment revision capture epoch deployment differs");
    }
    return existing;
  }
  let snapshot = retainedSnapshot;
  // create the retained zero snapshot before making its witness visible
  if (snapshot === null) {
    if (database.servingSnapshot === null) {
      throw new Error("adjustment revision capture epoch zero snapshot is unavailable");
    }
    await writeAdjustmentRevisionCaptureEpochAuthorityFile(
      snapshotPath,
      canonicalJsonBytes(database.servingSnapshot),
    );
    snapshot = await readAdjustmentRevisionCaptureEpochSnapshot({
      expectedGid: process.getegid(),
      expectedUid: process.geteuid(),
      path: snapshotPath,
    });
  }
  const unsigned = {
    activationKind: "inert_v14_pre_activation",
    archiveCommitOrdinal: "0",
    catalogFrontierSha256: adjustmentRevisionGenesisFrontier(),
    contractVersion: ADJUSTMENT_REVISION_CAPTURE_EPOCH_CONTRACT_VERSION,
    controlPlaneSha256: deployment.controlPlaneSha256,
    controlPlaneVersion: deployment.controlPlaneVersion,
    databaseMigrationHistorySha256: database.databaseMigrationHistorySha256,
    epochAt: snapshot.cutoffAt,
    servingSnapshotSha256: snapshot.snapshotSha256,
    sourceCommit: deployment.sourceCommit,
    sourceRelease: deployment.sourceRelease,
    sourceServerImageDigest: deployment.sourceServerImageDigest,
    sourceWebImageDigest: deployment.sourceWebImageDigest,
  };
  const witness = validateAdjustmentRevisionCaptureEpochWitness({
    ...unsigned,
    witnessSha256: sha256(canonicalJsonBytes(unsigned)),
  });
  await writeAdjustmentRevisionCaptureEpochAuthorityFile(path, canonicalJsonBytes(witness));
  const durable = await readAdjustmentRevisionCaptureEpochWitness({
    expectedGid: process.getegid(),
    expectedUid: process.geteuid(),
    path,
  });
  // preserve a racing first clock only for the same exact target identity
  if (!adjustmentRevisionCaptureEpochDeploymentMatches(durable, deployment) ||
    durable.servingSnapshotSha256 !== snapshot.snapshotSha256 ||
    durable.epochAt !== snapshot.cutoffAt) {
    throw new Error("adjustment revision capture epoch collision");
  }
  return durable;
}

// read one canonical single-link capture epoch from its fixed authority path
export async function readAdjustmentRevisionCaptureEpochWitness(input = {}) {
  requirePlainObject(input, "adjustment revision capture epoch read input");
  // reject unknown read controls while retaining optional fixed defaults
  if (Object.keys(input).some((key) =>
    !["expectedGid", "expectedUid", "optional", "path"].includes(key))) {
    throw new TypeError("adjustment revision capture epoch read input is invalid");
  }
  const path = resolve(input.path ?? ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH);
  const expectedUid = input.expectedUid ?? 0;
  const expectedGid = input.expectedGid ?? 0;
  let handle;
  try {
    handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  } catch (error) {
    // expose absence only to the create-once writer
    if (input.optional === true && error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
  try {
    const details = await handle.stat();
    // require the reviewed public root-owned immutable-file boundary
    if (!details.isFile() || details.nlink !== 1 || details.uid !== expectedUid ||
      details.gid !== expectedGid || (details.mode & 0o777) !== 0o644 ||
      details.size < 2 || details.size > ADJUSTMENT_REVISION_CAPTURE_EPOCH_MAXIMUM_BYTES) {
      throw new Error("adjustment revision capture epoch file is invalid");
    }
    const bytes = await handle.readFile();
    const value = validateAdjustmentRevisionCaptureEpochWitness(
      JSON.parse(bytes.toString("utf8")),
    );
    // reject alternate JSON representations at the root authority boundary
    if (!bytes.equals(canonicalJsonBytes(value))) {
      throw new Error("adjustment revision capture epoch file is not canonical");
    }
    return value;
  } finally {
    await handle.close();
  }
}

// read the actual canonical zero-frontier snapshot retained beside its witness
export async function readAdjustmentRevisionCaptureEpochSnapshot(input = {}) {
  requirePlainObject(input, "adjustment revision capture epoch snapshot input");
  // reject unknown read controls while retaining optional fixed defaults
  if (Object.keys(input).some((key) =>
    !["expectedGid", "expectedUid", "optional", "path"].includes(key))) {
    throw new TypeError("adjustment revision capture epoch snapshot input is invalid");
  }
  const path = resolve(input.path ?? ADJUSTMENT_REVISION_CAPTURE_EPOCH_SNAPSHOT_PATH);
  const expectedUid = input.expectedUid ?? 0;
  const expectedGid = input.expectedGid ?? 0;
  let handle;
  try {
    handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  } catch (error) {
    // expose absence only to the create-once writer
    if (input.optional === true && error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
  try {
    const details = await handle.stat();
    // require the same reviewed public root-owned immutable-file boundary
    if (!details.isFile() || details.nlink !== 1 || details.uid !== expectedUid ||
      details.gid !== expectedGid || (details.mode & 0o777) !== 0o644 ||
      details.size < 2 || details.size > ADJUSTMENT_REVISION_CAPTURE_EPOCH_MAXIMUM_BYTES) {
      throw new Error("adjustment revision capture epoch snapshot file is invalid");
    }
    const bytes = await handle.readFile();
    const snapshot = validateAdjustmentRevisionServingSnapshot(
      JSON.parse(bytes.toString("utf8")),
    );
    // retain only the actual canonical empty genesis snapshot
    if (snapshot.archiveCommitOrdinal !== "0" || snapshot.entryCount !== 0 ||
      snapshot.entries.length !== 0 ||
      snapshot.frontierSha256 !== adjustmentRevisionGenesisFrontier() ||
      !bytes.equals(canonicalJsonBytes(snapshot))) {
      throw new Error("adjustment revision capture epoch snapshot differs");
    }
    return snapshot;
  } finally {
    await handle.close();
  }
}

// create one root-authority file without replacing any prior identity
async function writeAdjustmentRevisionCaptureEpochAuthorityFile(path, bytes) {
  const directory = dirname(path);
  const canonicalDirectory = await realpath(directory);
  // refuse linked or noncanonical state parents before exclusive creation
  if (canonicalDirectory !== resolve(directory) || !(await lstat(directory)).isDirectory()) {
    throw new Error("adjustment revision capture epoch directory is invalid");
  }
  let handle;
  try {
    handle = await open(path,
      fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW,
      0o644);
    await handle.writeFile(bytes);
    await handle.chmod(0o644);
    await handle.sync();
  } catch (error) {
    // converge only a racing create-once authority file
    if (error?.code !== "EEXIST") {
      throw error;
    }
  } finally {
    await handle?.close();
  }
  await fsyncDirectory(directory);
}

// compare only immutable target activation fields across create-once retries
function adjustmentRevisionCaptureEpochDeploymentMatches(witness, deployment) {
  return witness.controlPlaneSha256 === deployment.controlPlaneSha256 &&
    witness.controlPlaneVersion === deployment.controlPlaneVersion &&
    witness.sourceCommit === deployment.sourceCommit &&
    witness.sourceRelease === deployment.sourceRelease &&
    witness.sourceServerImageDigest === deployment.sourceServerImageDigest &&
    witness.sourceWebImageDigest === deployment.sourceWebImageDigest;
}

// validate the root-derived target release identities only
function validateAdjustmentRevisionCaptureEpochDeployment(value) {
  requireExactKeys(value, [
    "controlPlaneSha256", "controlPlaneVersion", "sourceCommit", "sourceRelease",
    "sourceServerImageDigest", "sourceWebImageDigest",
  ], "adjustment revision capture epoch deployment");
  requireSha256(value.controlPlaneSha256,
    "adjustment revision capture epoch control plane");
  // reuse the complete witness validator after construction for remaining fields
  if (value.controlPlaneVersion !== "14" ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit) ||
    !/^\d{4}\.\d{2}\.\d{2}-[1-9][0-9]?$/u.test(value.sourceRelease) ||
    !/^sha256:[a-f0-9]{64}$/u.test(value.sourceServerImageDigest) ||
    !/^sha256:[a-f0-9]{64}$/u.test(value.sourceWebImageDigest)) {
    throw new TypeError("adjustment revision capture epoch deployment is invalid");
  }
  return value;
}

// validate the exact full-0020 database transaction and optional initial zero snapshot
function validateAdjustmentRevisionCaptureEpochDatabaseEnvelope(value) {
  requireExactKeys(value, ["databaseManifest", "payload", "transaction"],
    "adjustment revision capture epoch database envelope");
  requireExactKeys(value.databaseManifest, [
    "contract_version", "migration_checksums", "migration_history_sha256",
    "migration_names", "query_contract_sha256", "query_contract_version",
    "row_schema_sha256", "schema_migration", "site_key", "site_timezone",
  ], "adjustment revision capture epoch database manifest");
  const manifest = value.databaseManifest;
  const manifestHistorySha256 = Array.isArray(manifest.migration_names) &&
    Array.isArray(manifest.migration_checksums)
    ? sha256(Buffer.from(manifest.migration_names.map(
        // recompute the exact database ledger preimage independently
        (name, index) => `${name}:${manifest.migration_checksums[index]}`,
      ).join("\n")))
    : null;
  const reviewedLedger = ADJUSTMENT_CAPTURE_EPOCH_DATABASE_LEDGERS.find(
    // select only an exact reviewed full-history identity
    (ledger) => manifest.migration_history_sha256 === ledger.historySha256,
  );
  // require one disjoint complete reviewed ledger rather than a compatible prefix
  if (manifest.contract_version !== "adjustment-evaluation-export-manifest/v1" ||
    reviewedLedger === undefined || !Array.isArray(manifest.migration_names) ||
    manifest.migration_names.length !== reviewedLedger.count ||
    !Array.isArray(manifest.migration_checksums) ||
    manifest.migration_checksums.length !== reviewedLedger.count ||
    manifest.migration_names.at(-1) !== reviewedLedger.name ||
    manifest.migration_checksums.at(-1) !== reviewedLedger.checksum ||
    manifestHistorySha256 !== reviewedLedger.historySha256) {
    throw new Error("adjustment revision capture epoch database ledger differs");
  }
  requireExactKeys(value.transaction, [
    "created_at_utc", "idle_in_transaction_session_timeout", "isolation_level",
    "lock_timeout", "read_only", "statement_timeout",
  ], "adjustment revision capture epoch transaction");
  requireInstant(value.transaction.created_at_utc,
    "adjustment revision capture epoch transaction time");
  // require the same fixed read-only repeatable-read authority boundary
  if (value.transaction.idle_in_transaction_session_timeout !== "30s" ||
    value.transaction.isolation_level !== "repeatable read" ||
    value.transaction.lock_timeout !== "5s" || value.transaction.read_only !== "on" ||
    value.transaction.statement_timeout !== "5min") {
    throw new Error("adjustment revision capture epoch transaction differs");
  }
  // allow retry metadata to omit a mutable current-frontier snapshot
  if (value.payload === null) {
    return {
      databaseMigrationHistorySha256: reviewedLedger.historySha256,
      epochAt: value.transaction.created_at_utc,
      servingSnapshot: null,
    };
  }
  const snapshot = validateAdjustmentRevisionServingSnapshot(value.payload);
  // prove an empty authoritative genesis at this exact database server clock
  if (snapshot.archiveCommitOrdinal !== "0" || snapshot.entryCount !== 0 ||
    snapshot.entries.length !== 0 ||
    snapshot.frontierSha256 !== adjustmentRevisionGenesisFrontier() ||
    snapshot.cutoffAt !== value.transaction.created_at_utc) {
    throw new Error("adjustment revision capture epoch is not zero frontier");
  }
  return {
    databaseMigrationHistorySha256: reviewedLedger.historySha256,
    epochAt: value.transaction.created_at_utc,
    servingSnapshot: snapshot,
  };
}

// start one immutable cutoff-bound cold transfer from an exact database snapshot
export async function readAdjustmentRevisionColdTransferStart(input) {
  requireExactKeys(input, ["root", "servingSnapshot"],
    "adjustment revision cold transfer start input");
  const root = requireRevisionArchiveRoot(input.root);
  const snapshot = validateAdjustmentRevisionServingSnapshot(input.servingSnapshot);
  await requireRevisionColdDirectories(root);
  // bind the empty database snapshot only to the fixed frontier genesis
  if (snapshot.archiveCommitOrdinal === "0") {
    if (snapshot.frontierSha256 !== adjustmentRevisionGenesisFrontier()) {
      throw new Error("adjustment revision cold start genesis differs");
    }
  }
  const unsigned = {
    contractVersion: ADJUSTMENT_REVISION_COLD_START_CONTRACT_VERSION,
    servingSnapshot: snapshot,
    watermarkArchiveCommitOrdinal: snapshot.archiveCommitOrdinal,
    watermarkFrontierSha256: snapshot.frontierSha256,
  };
  const start = {
    ...unsigned,
    startSha256: sha256(canonicalJsonBytes(unsigned)),
  };
  // preserve the fixed wire ceiling independently of the database metadata cap
  if (canonicalJsonBytes(start).length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES) {
    throw new RangeError("adjustment revision cold transfer start is too large");
  }
  return Object.freeze(start);
}

// initialize only the fixed service-owned cold-reader directories
export async function initializeAdjustmentRevisionColdArchiveRoot(input) {
  requireExactKeys(input, ["expectedGid", "expectedUid", "root"],
    "adjustment revision cold archive initialization");
  const root = requireRevisionArchiveRoot(input.root);
  requireId(input.expectedUid, "adjustment revision cold archive uid");
  requireId(input.expectedGid, "adjustment revision cold archive gid");
  await requireOwnedPrivateDirectory(root, input.expectedUid, input.expectedGid);

  // create only the six children required by a genuine empty cold start
  for (const directory of [
    "revision-commit-receipts", "revision-frontier-successors",
    "revision-projections", "shadow-revision-capsules", "rain-control-states",
    "rain-control-state-stage-receipts",
  ]) {
    await ensureOwnedPrivateDirectory(
      join(root, directory),
      input.expectedUid,
      input.expectedGid,
    );
  }
  await requireRevisionColdDirectories(root);
  return Object.freeze({ root });
}

// read at most two successor payloads after one verified workstation cursor
export async function readAdjustmentRevisionColdPage(input) {
  requireExactKeys(input, [
    "afterArchiveCommitOrdinal", "afterFrontierSha256", "previousPageSha256", "root",
    "startSha256", "watermarkArchiveCommitOrdinal", "watermarkFrontierSha256",
  ], "adjustment revision cold page input");
  const root = requireRevisionArchiveRoot(input.root);
  requireUint64Text(input.afterArchiveCommitOrdinal,
    "adjustment revision cold page cursor ordinal");
  requireUint64Text(input.watermarkArchiveCommitOrdinal,
    "adjustment revision cold page watermark ordinal");
  for (const [value, label] of [
    [input.afterFrontierSha256, "adjustment revision cold page cursor frontier"],
    [input.previousPageSha256, "adjustment revision cold page predecessor"],
    [input.startSha256, "adjustment revision cold page start"],
    [input.watermarkFrontierSha256, "adjustment revision cold page watermark frontier"],
  ]) {
    requireSha256(value, label);
  }
  const afterOrdinal = BigInt(input.afterArchiveCommitOrdinal);
  const watermarkOrdinal = BigInt(input.watermarkArchiveCommitOrdinal);
  // reject a cursor beyond the immutable transfer watermark
  if (afterOrdinal > watermarkOrdinal) {
    throw new RangeError("adjustment revision cold page cursor exceeds watermark");
  }
  await requireRevisionColdDirectories(root);
  // bind only the reserved zero cursor locally; later cursors are workstation-verified
  if (input.afterArchiveCommitOrdinal === "0" &&
    input.afterFrontierSha256 !== adjustmentRevisionGenesisFrontier()) {
    throw new Error("adjustment revision cold page genesis differs");
  }
  // require an already-complete cursor to equal both watermark coordinates
  if (afterOrdinal === watermarkOrdinal &&
    input.afterFrontierSha256 !== input.watermarkFrontierSha256) {
    throw new Error("adjustment revision cold page completed cursor differs");
  }
  // require the start hash as the first page predecessor
  if (afterOrdinal === 0n && input.previousPageSha256 !== input.startSha256) {
    throw new Error("adjustment revision cold first page predecessor differs");
  }
  const entries = [];
  let nextArchiveCommitOrdinal = input.afterArchiveCommitOrdinal;
  let nextFrontierSha256 = input.afterFrontierSha256;
  // fill no more than the fixed two payload slots
  while (entries.length < ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_PAYLOADS &&
    BigInt(nextArchiveCommitOrdinal) < watermarkOrdinal) {
    const group = await readStoredRevisionColdGroup(root, nextFrontierSha256);
    const firstReceipt = group.receipts[0];
    const lastReceipt = group.receipts.at(-1);
    // require the whole direct group within the selected immutable watermark
    if (firstReceipt.predecessorFrontierSha256 !== nextFrontierSha256 ||
      BigInt(firstReceipt.archiveCommitOrdinal) <= BigInt(nextArchiveCommitOrdinal) ||
      BigInt(lastReceipt.archiveCommitOrdinal) > watermarkOrdinal) {
      throw new Error("adjustment revision cold successor differs");
    }
    const payload = await readStoredRevisionPayload(root, firstReceipt);
    const auxiliary = group.batch
      ? await readStoredRevisionBatchAuxiliary(root, group.receipts, payload)
      : await readStoredRevisionAuxiliary(
        root,
        firstReceipt,
        payload,
        group.successors[0],
      );
    const rainControl = payload.kind === "adjustment-rain-gate-control-projection/v3"
      ? await readStoredRainControlStateAuxiliary(root, payload)
      : null;
    const nextEntry = group.batch
      ? {
        payload,
        publication: auxiliary.publication,
        receipts: group.receipts,
        stageReceipt: auxiliary.stageReceipt,
        successors: group.successors,
      }
      : {
        payload,
        publication: auxiliary.publication,
        ...(rainControl === null ? {} : rainControl),
        receipt: firstReceipt,
        stageReceipt: auxiliary.stageReceipt,
        successor: group.successors[0],
      };
    const candidateEntries = [...entries, nextEntry];
    const candidate = buildAdjustmentRevisionColdPage({
      ...input,
      entries: candidateEntries,
      nextArchiveCommitOrdinal: lastReceipt.archiveCommitOrdinal,
      nextFrontierSha256: lastReceipt.frontierSha256,
    });
    // defer only the second payload when the aggregate page would cross its ceiling
    if (canonicalJsonBytes(candidate).length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES) {
      if (entries.length === 0) {
        throw new RangeError("adjustment revision cold payload exceeds page limit");
      }
      break;
    }
    entries.push(candidateEntries.at(-1));
    nextArchiveCommitOrdinal = lastReceipt.archiveCommitOrdinal;
    nextFrontierSha256 = lastReceipt.frontierSha256;
  }
  const page = buildAdjustmentRevisionColdPage({
    ...input,
    entries,
    nextArchiveCommitOrdinal,
    nextFrontierSha256,
  });
  validateAdjustmentRevisionColdPage(page);
  return Object.freeze(page);
}

// validate one cutoff-bound transfer start and its exact database snapshot
export function validateAdjustmentRevisionColdTransferStart(value) {
  requireExactKeys(value, [
    "contractVersion", "servingSnapshot", "startSha256",
    "watermarkArchiveCommitOrdinal", "watermarkFrontierSha256",
  ], "adjustment revision cold transfer start");
  const snapshot = validateAdjustmentRevisionServingSnapshot(value.servingSnapshot);
  const unsigned = {
    contractVersion: value.contractVersion,
    servingSnapshot: snapshot,
    watermarkArchiveCommitOrdinal: value.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: value.watermarkFrontierSha256,
  };
  // bind the transfer watermark and identity to the exact snapshot bytes
  if (value.contractVersion !== ADJUSTMENT_REVISION_COLD_START_CONTRACT_VERSION ||
    value.watermarkArchiveCommitOrdinal !== snapshot.archiveCommitOrdinal ||
    value.watermarkFrontierSha256 !== snapshot.frontierSha256 ||
    value.startSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES) {
    throw new Error("adjustment revision cold transfer start differs");
  }
  return Object.freeze(value);
}

// validate one bounded successor page without trusting transport metadata
export function validateAdjustmentRevisionColdPage(value) {
  requireExactKeys(value, [
    "afterArchiveCommitOrdinal", "afterFrontierSha256", "contractVersion", "entries",
    "eof", "nextArchiveCommitOrdinal", "nextFrontierSha256", "pageSha256",
    "previousPageSha256", "startSha256", "watermarkArchiveCommitOrdinal",
    "watermarkFrontierSha256",
  ], "adjustment revision cold page");
  // require the fixed payload count and all cursor identity fields
  if (value.contractVersion !== ADJUSTMENT_REVISION_COLD_PAGE_CONTRACT_VERSION ||
    !Array.isArray(value.entries) ||
    value.entries.length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_PAYLOADS ||
    typeof value.eof !== "boolean") {
    throw new TypeError("adjustment revision cold page is invalid");
  }
  for (const field of [
    "afterArchiveCommitOrdinal", "nextArchiveCommitOrdinal",
    "watermarkArchiveCommitOrdinal",
  ]) {
    requireUint64Text(value[field], `adjustment revision cold page ${field}`);
  }
  for (const field of [
    "afterFrontierSha256", "nextFrontierSha256", "pageSha256",
    "previousPageSha256", "startSha256", "watermarkFrontierSha256",
  ]) {
    requireSha256(value[field], `adjustment revision cold page ${field}`);
  }
  let priorOrdinal = BigInt(value.afterArchiveCommitOrdinal);
  let priorFrontierSha256 = value.afterFrontierSha256;
  // verify every direct server successor and its exact cold payload bytes
  for (const entry of value.entries) {
    const parts = revisionColdEntryParts(entry);
    validateRevisionColdPayload(entry.payload, parts.receipts[0]);
    if (parts.batch) {
      validateRevisionColdBatchAuxiliary(entry);
    } else {
      validateRevisionColdAuxiliary(entry);
    }
    // reject gaps, rollback or a partial grouped body at the watermark
    for (const receipt of parts.receipts) {
      validateRevisionCommitReceiptIdentity(receipt);
      const ordinal = BigInt(receipt.archiveCommitOrdinal);
      if (receipt.predecessorFrontierSha256 !== priorFrontierSha256 ||
        ordinal <= priorOrdinal || ordinal > BigInt(value.watermarkArchiveCommitOrdinal)) {
        throw new Error("adjustment revision cold page chain differs");
      }
      priorOrdinal = ordinal;
      priorFrontierSha256 = receipt.frontierSha256;
    }
  }
  const atWatermark = value.nextArchiveCommitOrdinal ===
      value.watermarkArchiveCommitOrdinal &&
    value.nextFrontierSha256 === value.watermarkFrontierSha256;
  const unsigned = adjustmentRevisionColdPageUnsigned(value);
  // bind next cursor, EOF authority, page predecessor and aggregate wire ceiling
  if (value.nextArchiveCommitOrdinal !== priorOrdinal.toString() ||
    value.nextFrontierSha256 !== priorFrontierSha256 || value.eof !== atWatermark ||
    (!atWatermark && priorOrdinal >= BigInt(value.watermarkArchiveCommitOrdinal)) ||
    value.pageSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES ||
    (value.entries.length === 0 && !value.eof)) {
    throw new Error("adjustment revision cold page identity differs");
  }
  return Object.freeze(value);
}

// project one successful successor page into nonduplicating cold graph members
export function buildAdjustmentRevisionColdGraphSegment(input) {
  requireExactKeys(input, ["page", "start"], "adjustment revision cold graph input");
  const page = validateAdjustmentRevisionColdPage(input.page);
  const firstPage = page.previousPageSha256 === page.startSha256;
  let start = null;
  // include the complete database snapshot exactly once at the first page
  if (firstPage) {
    start = validateAdjustmentRevisionColdTransferStart(input.start);
    // bind the archived start to this page's immutable watermark
    if (start.startSha256 !== page.startSha256 ||
      start.watermarkArchiveCommitOrdinal !== page.watermarkArchiveCommitOrdinal ||
      start.watermarkFrontierSha256 !== page.watermarkFrontierSha256) {
      throw new Error("adjustment revision cold graph start differs");
    }
  // forbid re-archiving the same start member on later pages
  } else if (input.start !== null) {
    throw new TypeError("adjustment revision later page must not duplicate its start member");
  }
  const checkpointEntries = revisionColdCheckpointEntries(page);
  const memberRootSha256 = successfulRevisionMemberRoot(checkpointEntries,
    start === null ? null : sha256(canonicalJsonBytes(start)));
  const checkpoint = {
    afterArchiveCommitOrdinal: page.afterArchiveCommitOrdinal,
    afterFrontierSha256: page.afterFrontierSha256,
    contractVersion: ADJUSTMENT_REVISION_COLD_CHECKPOINT_CONTRACT_VERSION,
    entries: checkpointEntries,
    eof: page.eof,
    memberRootSha256,
    nextArchiveCommitOrdinal: page.nextArchiveCommitOrdinal,
    nextFrontierSha256: page.nextFrontierSha256,
    pageSha256: page.pageSha256,
    previousPageSha256: page.previousPageSha256,
    startMemberSha256: start === null ? null : sha256(canonicalJsonBytes(start)),
    startSha256: page.startSha256,
    watermarkArchiveCommitOrdinal: page.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: page.watermarkFrontierSha256,
  };
  validateAdjustmentRevisionColdCheckpoint(checkpoint);
  const members = [{
    identitySha256: page.pageSha256,
    kind: ADJUSTMENT_REVISION_COLD_CHECKPOINT_CONTRACT_VERSION,
    payload: canonicalJsonBytes(checkpoint),
  }];
  const crossLinks = [];
  // include the exact cutoff-bound serving snapshot only on the first page
  if (start !== null) {
    members.push({
      identitySha256: start.startSha256,
      kind: ADJUSTMENT_REVISION_COLD_START_CONTRACT_VERSION,
      payload: canonicalJsonBytes(start),
    });
    crossLinks.push({
      fromIdentitySha256: page.pageSha256,
      relation: "binds_transfer_start",
      toIdentitySha256: start.startSha256,
    });
  }
  // include every receipt, value and auxiliary proof as exact distinct members
  for (const entry of page.entries) {
    const parts = revisionColdEntryParts(entry);
    const capsule = parts.receipts[0].projectionKind === "shadow_prediction"
      ? parseShadowRevisionCapsule(Buffer.from(entry.payload.bytesBase64, "base64"))
      : null;
    const comparatorBytes = capsule?.contractVersion ===
      "adjustment-shadow-revision-capsule/v2"
      ? decodeCanonicalBase64(capsule.comparatorBase64,
          "adjustment shadow incumbent comparator")
      : null;
    const publicationIdentitySha256 = adjustmentRevisionColdPublicationIdentity(
      entry.publication,
    );
    members.push(
      {
        identitySha256: entry.payload.identitySha256,
        kind: entry.payload.kind,
        payload: Buffer.from(entry.payload.bytesBase64, "base64"),
      },
      {
        identitySha256: entry.stageReceipt.stageReceiptSha256,
        kind: entry.stageReceipt.contractVersion,
        payload: canonicalJsonBytes(entry.stageReceipt),
      },
      {
        identitySha256: publicationIdentitySha256,
        kind: entry.publication.value.contractVersion,
        payload: canonicalJsonBytes(entry.publication.value),
      },
    );
    // archive the exact monthly state and its first availability receipt once
    if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
      members.push(
        {
          identitySha256: entry.rainControlState.identitySha256,
          kind: entry.rainControlState.kind,
          payload: Buffer.from(entry.rainControlState.bytesBase64, "base64"),
        },
        {
          identitySha256: entry.rainControlStateStageReceipt.stageReceiptSha256,
          kind: entry.rainControlStateStageReceipt.contractVersion,
          payload: canonicalJsonBytes(entry.rainControlStateStageReceipt),
        },
      );
    }
    // archive the actual incumbent comparator as its own semantic graph member
    if (comparatorBytes !== null) {
      members.push({
        identitySha256: capsule.comparatorSha256,
        kind: "adjustment-shadow-incumbent-comparator/v1",
        payload: comparatorBytes,
      });
    }
    // bind every grouped receipt to the one deduplicated body and auxiliary members
    for (const [index, receipt] of parts.receipts.entries()) {
      const successor = parts.successors[index];
      const successorSha256 = sha256(canonicalJsonBytes(successor));
      members.push(
        {
          identitySha256: receipt.receiptSha256,
          kind: receipt.contractVersion,
          payload: canonicalJsonBytes(receipt),
        },
        {
          identitySha256: successorSha256,
          kind: successor.contractVersion,
          payload: canonicalJsonBytes(successor),
        },
      );
      crossLinks.push(
        {
          fromIdentitySha256: page.pageSha256,
          relation: "contains_receipt",
          toIdentitySha256: receipt.receiptSha256,
        },
        {
          fromIdentitySha256: receipt.receiptSha256,
          relation: "binds_payload",
          toIdentitySha256: entry.payload.identitySha256,
        },
        {
          fromIdentitySha256: receipt.receiptSha256,
          relation: "binds_stage_receipt",
          toIdentitySha256: entry.stageReceipt.stageReceiptSha256,
        },
        {
          fromIdentitySha256: receipt.receiptSha256,
          relation: entry.publication.disposition === "published"
            ? "binds_publish_receipt" : "binds_terminal_gap",
          toIdentitySha256: publicationIdentitySha256,
        },
        {
          fromIdentitySha256: receipt.receiptSha256,
          relation: "binds_successor",
          toIdentitySha256: successorSha256,
        },
      );
      // crosslink only the additive actual comparator member to its receipt
      if (comparatorBytes !== null) {
        crossLinks.push({
          fromIdentitySha256: receipt.receiptSha256,
          relation: "binds_incumbent_comparator",
          toIdentitySha256: capsule.comparatorSha256,
        });
      }
      // bind the control receipt to both genuine public state members
      if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
        crossLinks.push(
          {
            fromIdentitySha256: receipt.receiptSha256,
            relation: "binds_rain_control_state",
            toIdentitySha256: entry.rainControlState.identitySha256,
          },
          {
            fromIdentitySha256: receipt.receiptSha256,
            relation: "binds_rain_control_state_stage_receipt",
            toIdentitySha256:
              entry.rainControlStateStageReceipt.stageReceiptSha256,
          },
        );
      }
    }
  }
  return Object.freeze({ crossLinks: Object.freeze(crossLinks),
    members: Object.freeze(members) });
}

// validate one compact successful-page checkpoint and its actual member hashes
export function validateAdjustmentRevisionColdCheckpoint(value) {
  requireExactKeys(value, [
    "afterArchiveCommitOrdinal", "afterFrontierSha256", "contractVersion", "entries",
    "eof", "memberRootSha256", "nextArchiveCommitOrdinal", "nextFrontierSha256",
    "pageSha256", "previousPageSha256", "startMemberSha256", "startSha256",
    "watermarkArchiveCommitOrdinal", "watermarkFrontierSha256",
  ], "adjustment revision cold checkpoint");
  // retain only the exact two-slot checkpoint grammar
  if (value.contractVersion !== ADJUSTMENT_REVISION_COLD_CHECKPOINT_CONTRACT_VERSION ||
    !Array.isArray(value.entries) ||
    value.entries.length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_PAYLOADS ||
    typeof value.eof !== "boolean") {
    throw new TypeError("adjustment revision cold checkpoint is invalid");
  }
  for (const field of [
    "afterArchiveCommitOrdinal", "nextArchiveCommitOrdinal",
    "watermarkArchiveCommitOrdinal",
  ]) {
    requireUint64Text(value[field], `adjustment revision cold checkpoint ${field}`);
  }
  for (const field of [
    "afterFrontierSha256", "memberRootSha256", "nextFrontierSha256", "pageSha256",
    "previousPageSha256", "startSha256", "watermarkFrontierSha256",
  ]) {
    requireSha256(value[field], `adjustment revision cold checkpoint ${field}`);
  }
  // accept a start member only on the first page checkpoint
  if (value.startMemberSha256 !== null) {
    requireSha256(value.startMemberSha256,
      "adjustment revision cold checkpoint start member");
  }
  for (const entry of value.entries) {
    const batch = Object.hasOwn(entry, "receiptMemberSha256s");
    for (const field of [
      "payloadIdentitySha256", "publicationIdentitySha256", "publicationMemberSha256",
      "stageReceiptMemberSha256", "stageReceiptSha256",
    ]) {
      requireSha256(entry[field], `adjustment revision cold checkpoint ${field}`);
    }
    // retain only the two closed publication outcomes
    if (!["published", "committed_unpublished_gap"]
      .includes(entry.publicationDisposition)) {
      throw new TypeError("adjustment revision cold checkpoint publication is invalid");
    }
    // select the additive grouped checkpoint without weakening the v1 shape
    if (batch) {
      requireExactKeys(entry, [
        "payloadIdentitySha256", "payloadKind", "publicationDisposition",
        "publicationIdentitySha256", "publicationMemberSha256",
        "receiptMemberSha256s", "receiptSha256s", "stageReceiptMemberSha256",
        "stageReceiptSha256", "successorMemberSha256s", "successorSha256s",
      ], "adjustment revision cold batch checkpoint entry");
      const arrays = [entry.receiptMemberSha256s, entry.receiptSha256s,
        entry.successorMemberSha256s, entry.successorSha256s];
      if (!["adjustment-revision-batch-projection/v2",
        "adjustment-rain-fixed-gauge-target-projection/v1"]
        .includes(entry.payloadKind) ||
        arrays.some((items) => !Array.isArray(items) || items.length < 1 ||
          items.length > ADJUSTMENT_REVISION_BATCH_MAXIMUM_RECEIPTS) ||
        arrays.some((items) => items.length !== arrays[0].length) ||
        arrays.flat().some((identity) => !/^[a-f0-9]{64}$/u.test(identity)) ||
        canonicalJson(entry.successorMemberSha256s) !==
          canonicalJson(entry.successorSha256s)) {
        throw new TypeError("adjustment revision cold batch checkpoint member is invalid");
      }
    } else {
      const comparator = entry.payloadKind === "adjustment-shadow-revision-capsule/v2";
      const rainControl = entry.payloadKind ===
        "adjustment-rain-gate-control-projection/v3";
      requireExactKeys(entry, [
        ...(comparator ? ["comparatorMemberSha256"] : []),
        "payloadIdentitySha256", "payloadKind", "publicationDisposition",
        "publicationIdentitySha256", "publicationMemberSha256",
        ...(rainControl ? ["rainControlStateIdentitySha256",
          "rainControlStateSha256", "rainControlStateStageReceiptMemberSha256",
          "rainControlStateStageReceiptSha256"] : []),
        "receiptMemberSha256", "receiptSha256", "stageReceiptMemberSha256",
        "stageReceiptSha256", "successorMemberSha256", "successorSha256",
      ], "adjustment revision cold checkpoint entry");
      for (const field of [
        "receiptMemberSha256", "receiptSha256", "successorMemberSha256",
        "successorSha256", ...(comparator ? ["comparatorMemberSha256"] : []),
        ...(rainControl ? ["rainControlStateIdentitySha256",
          "rainControlStateSha256", "rainControlStateStageReceiptMemberSha256",
          "rainControlStateStageReceiptSha256"] : []),
      ]) {
        requireSha256(entry[field], `adjustment revision cold checkpoint ${field}`);
      }
      if (!["adjustment-revision-projection/v1",
        "adjustment-rain-gate-feature-projection/v2",
        "adjustment-rain-gate-control-projection/v3",
        "adjustment-rain-fixed-gauge-target-projection/v1",
      "adjustment-temperature-native-source-projection/v2",
        "adjustment-shadow-revision-capsule/v1", "adjustment-shadow-revision-capsule/v2"]
        .includes(entry.payloadKind) ||
        entry.successorMemberSha256 !== entry.successorSha256) {
        throw new TypeError("adjustment revision cold checkpoint member is invalid");
      }
    }
  }
  const memberRootSha256 = successfulRevisionMemberRoot(
    value.entries,
    value.startMemberSha256,
  );
  // reject a checkpoint detached from its exact complete member population
  if (value.memberRootSha256 !== memberRootSha256 ||
    (value.previousPageSha256 === value.startSha256) !==
      (value.startMemberSha256 !== null)) {
    throw new Error("adjustment revision cold checkpoint member root differs");
  }
  return Object.freeze(value);
}

// hash the actual successful-page member bytes without repeating payloads
function successfulRevisionMemberRoot(entries, startMemberSha256) {
  return sha256(canonicalJsonBytes({
    entries: entries.map(
      // bind every file-equivalent member hash in receipt order
      (entry) => ({
        ...(Object.hasOwn(entry, "comparatorMemberSha256")
          ? { comparatorMemberSha256: entry.comparatorMemberSha256 }
          : {}),
        payloadMemberSha256: entry.payloadIdentitySha256,
        publicationDisposition: entry.publicationDisposition,
        publicationIdentitySha256: entry.publicationIdentitySha256,
        publicationMemberSha256: entry.publicationMemberSha256,
        ...(Object.hasOwn(entry, "rainControlStateIdentitySha256")
          ? {
              rainControlStateMemberSha256: entry.rainControlStateIdentitySha256,
              rainControlStateStageReceiptMemberSha256:
                entry.rainControlStateStageReceiptMemberSha256,
            }
          : {}),
        ...(Object.hasOwn(entry, "receiptMemberSha256")
          ? { receiptMemberSha256: entry.receiptMemberSha256 }
          : { receiptMemberSha256s: entry.receiptMemberSha256s }),
        stageReceiptMemberSha256: entry.stageReceiptMemberSha256,
        ...(Object.hasOwn(entry, "successorMemberSha256")
          ? { successorMemberSha256: entry.successorMemberSha256 }
          : { successorMemberSha256s: entry.successorMemberSha256s }),
      }),
    ),
    startMemberSha256,
  }));
}

// derive exact graph-member hashes from one validated successful page
function revisionColdCheckpointEntries(pageValue) {
  const page = validateAdjustmentRevisionColdPage(pageValue);
  return page.entries.map(
    // retain every exact member identity and file hash without duplicating value bytes
    (entry) => {
      const parts = revisionColdEntryParts(entry);
      const common = {
        ...(entry.payload.kind === "adjustment-shadow-revision-capsule/v2"
          ? { comparatorMemberSha256: parseShadowRevisionCapsule(
              Buffer.from(entry.payload.bytesBase64, "base64"),
            ).comparatorSha256 }
          : {}),
        ...(entry.payload.kind === "adjustment-rain-gate-control-projection/v3"
          ? {
              rainControlStateIdentitySha256: entry.rainControlState.identitySha256,
              rainControlStateSha256: entry.rainControlState.stateSha256,
              rainControlStateStageReceiptMemberSha256:
                sha256(canonicalJsonBytes(entry.rainControlStateStageReceipt)),
              rainControlStateStageReceiptSha256:
                entry.rainControlStateStageReceipt.stageReceiptSha256,
            }
          : {}),
        payloadIdentitySha256: entry.payload.identitySha256,
        payloadKind: entry.payload.kind,
        publicationDisposition: entry.publication.disposition,
        publicationIdentitySha256: adjustmentRevisionColdPublicationIdentity(
          entry.publication,
        ),
        publicationMemberSha256: sha256(canonicalJsonBytes(entry.publication.value)),
        stageReceiptMemberSha256: sha256(canonicalJsonBytes(entry.stageReceipt)),
        stageReceiptSha256: entry.stageReceipt.stageReceiptSha256,
      };
      // keep the frozen v1 single-receipt checkpoint shape unchanged
      if (!parts.batch) {
        return {
          ...common,
          receiptMemberSha256: sha256(canonicalJsonBytes(entry.receipt)),
          receiptSha256: entry.receipt.receiptSha256,
          successorMemberSha256: sha256(canonicalJsonBytes(entry.successor)),
          successorSha256: sha256(canonicalJsonBytes(entry.successor)),
        };
      }
      return {
        ...common,
        receiptMemberSha256s: parts.receipts.map(
          // retain every actual receipt member in global order
          (receipt) => sha256(canonicalJsonBytes(receipt)),
        ),
        receiptSha256s: parts.receipts.map(
          // retain every server receipt identity in the same order
          (receipt) => receipt.receiptSha256,
        ),
        successorMemberSha256s: parts.successors.map(
          // retain every embedded successor member in the same order
          (successor) => sha256(canonicalJsonBytes(successor)),
        ),
        successorSha256s: parts.successors.map(
          // use exact canonical successor bytes as their cold identities
          (successor) => sha256(canonicalJsonBytes(successor)),
        ),
      };
    },
  );
}

// select the semantic publication identity without relabelling actual member bytes
function adjustmentRevisionColdPublicationIdentity(publication) {
  // retain the producer-issued publication identity on successful pages
  if (publication.disposition === "published") {
    return requireSha256(publication.value.publishReceiptSha256,
      "adjustment revision cold publish receipt identity");
  }
  // retain the permanent gap identity on terminal ordinal pages
  if (publication.disposition === "committed_unpublished_gap") {
    return requireSha256(publication.value.gapSha256,
      "adjustment revision cold terminal gap identity");
  }
  throw new TypeError("adjustment revision cold publication identity is invalid");
}

// acknowledge one archived successful page before exact hot redundancy retirement
export async function acknowledgeAdjustmentRevisionColdPage(input, options = {}) {
  validateAdjustmentRevisionCustodyRequest(input, "graphManifestSha256");
  return await acknowledgeAdjustmentRevisionColdCustody(input, options, {
    contractVersion: ADJUSTMENT_REVISION_CUSTODY_ACK_CONTRACT_VERSION,
    identityField: "graphManifestSha256",
    validate: validateAdjustmentRevisionColdCustodyAcknowledgement,
  });
}

// acknowledge one locally packed page before exact hot redundancy retirement
export async function acknowledgeAdjustmentRevisionColdCustodyCheckpoint(
  input,
  options = {},
) {
  validateAdjustmentRevisionCustodyRequest(input, "custodyCheckpointSha256");
  return await acknowledgeAdjustmentRevisionColdCustody(input, options, {
    contractVersion: ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION,
    identityField: "custodyCheckpointSha256",
    validate: validateAdjustmentRevisionColdCustodyAcknowledgementV2,
  });
}

// validate one compact shadow metadata proof derived from exact cold capsules
export function validateAdjustmentShadowMetadataCustodyProof(value) {
  requireExactKeys(value, [
    "acknowledgementSha256", "archiveCommitOrdinal", "authority", "contractVersion",
    "custodyCheckpointSha256", "entries", "frontierSha256", "memberRootSha256",
    "pageSha256", "preparedAt", "proofSha256",
  ], "adjustment shadow metadata custody proof");
  if (value.contractVersion !== ADJUSTMENT_SHADOW_METADATA_CUSTODY_PROOF_CONTRACT_VERSION ||
    value.authority !== "shadow_metadata_custody_only" ||
    !Array.isArray(value.entries) || value.entries.length < 1 || value.entries.length > 2) {
    throw new TypeError("adjustment shadow metadata custody proof is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal,
    "adjustment shadow metadata custody ordinal");
  requireInstant(value.preparedAt, "adjustment shadow metadata custody time");
  for (const field of [
    "acknowledgementSha256", "custodyCheckpointSha256", "frontierSha256",
    "memberRootSha256", "pageSha256", "proofSha256",
  ]) {
    requireSha256(value[field], `adjustment shadow metadata custody ${field}`);
  }
  const entries = value.entries.map(
    // validate every capsule-derived compact row identity
    (entry) => validateAdjustmentShadowMetadataCustodyEntry(entry),
  );
  const ordered = [...entries].sort((left, right) =>
    left.registrationSha256.localeCompare(right.registrationSha256) ||
      left.predictionSha256.localeCompare(right.predictionSha256));
  const unsigned = { ...value };
  delete unsigned.proofSha256;
  // require stable entry order and an exact self-address
  if (canonicalJson(entries) !== canonicalJson(ordered) ||
    value.proofSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new Error("adjustment shadow metadata custody proof differs");
  }
  return Object.freeze(value);
}

// validate one exact shadow compact row and its global revision receipt
function validateAdjustmentShadowMetadataCustodyEntry(value) {
  requireExactKeys(value, [
    "archiveCommitOrdinal", "archiveCommittedAt", "bodyByteCount", "capsuleSha256",
    "dueKey", "frontierSha256", "inputSha256", "issuedAt", "maxValidAt",
    "minValidAt", "predictionBodySha256", "predictionCommittedAt",
    "predictionSchemaSha256", "predictionSha256", "predecessorFrontierSha256",
    "receiptSha256", "registrationSha256", "rowCount", "sourceReceiptSha256",
    "stageReceiptSha256",
  ], "adjustment shadow metadata custody entry");
  requireUint64Text(value.archiveCommitOrdinal,
    "adjustment shadow metadata archive ordinal");
  for (const field of [
    "archiveCommittedAt", "issuedAt", "maxValidAt", "minValidAt",
    "predictionCommittedAt",
  ]) {
    requireInstant(value[field], `adjustment shadow metadata ${field}`);
  }
  for (const field of [
    "capsuleSha256", "frontierSha256", "inputSha256", "predictionBodySha256",
    "predictionSchemaSha256", "predictionSha256", "predecessorFrontierSha256",
    "receiptSha256", "registrationSha256", "sourceReceiptSha256",
    "stageReceiptSha256",
  ]) {
    requireSha256(value[field], `adjustment shadow metadata ${field}`);
  }
  if (!Number.isSafeInteger(value.bodyByteCount) || value.bodyByteCount < 2 ||
    value.bodyByteCount > ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES || !Number.isSafeInteger(value.rowCount) ||
    value.rowCount < 1 || value.rowCount > 168 ||
    !/^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u.test(value.dueKey) ||
    Date.parse(value.issuedAt) > Date.parse(value.predictionCommittedAt) ||
    Date.parse(value.predictionCommittedAt) > Date.parse(value.archiveCommittedAt)) {
    throw new TypeError("adjustment shadow metadata custody entry is invalid");
  }
  return Object.freeze(value);
}

// validate one exact owner-side metadata finalization prepared before mutation
function validateAdjustmentShadowMetadataFinalization(value) {
  requireExactKeys(value, [
    "coldCommitSha256", "expectedPreviousMetadataRootSha256", "finalDisposition",
    "fromCommittedAt", "generation", "maintenanceAnchorSha256",
    "metadataManifestSha256", "newMetadataRootSha256", "registrationSha256",
    "rowCount", "throughCommittedAt",
  ], "adjustment shadow metadata finalization");
  // allow only bounded retain-only native operations
  if (value.finalDisposition !== "retain" ||
    !Number.isSafeInteger(value.generation) || value.generation < 1 ||
    !Number.isSafeInteger(value.rowCount) || value.rowCount < 1 || value.rowCount > 2) {
    throw new TypeError("adjustment shadow metadata finalization is invalid");
  }
  requireInstant(value.fromCommittedAt,
    "adjustment shadow metadata finalization fromCommittedAt");
  requireInstant(value.throughCommittedAt,
    "adjustment shadow metadata finalization throughCommittedAt");
  // validate every database and cold-custody identity
  for (const field of [
    "coldCommitSha256", "expectedPreviousMetadataRootSha256",
    "maintenanceAnchorSha256", "metadataManifestSha256", "newMetadataRootSha256",
    "registrationSha256",
  ]) {
    requireSha256(value[field], `adjustment shadow metadata finalization ${field}`);
  }
  // bind the successor root to the immutable native function preimage
  const expectedRoot = sha256([
    value.expectedPreviousMetadataRootSha256,
    value.metadataManifestSha256,
    String(value.generation),
    String(value.rowCount),
    value.throughCommittedAt,
    value.coldCommitSha256,
    value.maintenanceAnchorSha256,
  ].join("\n"));
  if (Date.parse(value.fromCommittedAt) > Date.parse(value.throughCommittedAt) ||
    value.newMetadataRootSha256 !== expectedRoot) {
    throw new Error("adjustment shadow metadata finalization root differs");
  }
  return Object.freeze(value);
}

// validate one durable preparation that precedes the owner transaction
export function validateAdjustmentShadowMetadataCustodyPreparation(value) {
  requireExactKeys(value, [
    "authority", "contractVersion", "finalizations", "preparedAt",
    "preparationSha256", "proofSha256",
  ], "adjustment shadow metadata custody preparation");
  // close authority, contract and bounded argument count together
  if (value.authority !== "shadow_metadata_transfer" ||
    value.contractVersion !==
      ADJUSTMENT_SHADOW_METADATA_CUSTODY_PREPARATION_CONTRACT_VERSION ||
    !Array.isArray(value.finalizations) || value.finalizations.length < 1 ||
    value.finalizations.length > 2) {
    throw new TypeError("adjustment shadow metadata custody preparation is invalid");
  }
  requireInstant(value.preparedAt, "adjustment shadow metadata preparation time");
  requireSha256(value.proofSha256, "adjustment shadow metadata preparation proof");
  requireSha256(value.preparationSha256,
    "adjustment shadow metadata preparation identity");
  const finalizations = value.finalizations.map(
    // validate every closed native function argument
    (finalization) => validateAdjustmentShadowMetadataFinalization(finalization),
  );
  const ordered = [...finalizations].sort(
    // stabilize preparation order by registration identity
    (left, right) => left.registrationSha256.localeCompare(right.registrationSha256),
  );
  const unsigned = { ...value };
  delete unsigned.preparationSha256;
  // require stable registration order and one exact self-address
  if (canonicalJson(finalizations) !== canonicalJson(ordered) ||
    new Set(finalizations.map(
      // count each prepared registration identity
      (entry) => entry.registrationSha256,
    )).size !==
      finalizations.length ||
    value.preparationSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new Error("adjustment shadow metadata custody preparation differs");
  }
  return Object.freeze(value);
}

// validate one path-free metadata custody status envelope
export function validateAdjustmentShadowMetadataCustodyStatus(value) {
  requireExactKeys(value, ["contractVersion", "preparation", "proof"],
    "adjustment shadow metadata custody status");
  // require the one path-free status version
  if (value.contractVersion !== ADJUSTMENT_SHADOW_METADATA_CUSTODY_STATUS_CONTRACT_VERSION) {
    throw new TypeError("adjustment shadow metadata custody status is invalid");
  }
  const proof = value.proof === null
    ? null
    : validateAdjustmentShadowMetadataCustodyProof(value.proof);
  const preparation = value.preparation === null
    ? null
    : validateAdjustmentShadowMetadataCustodyPreparation(value.preparation);
  // forbid orphaned preparation state or a cross-proof substitution
  if ((proof === null && preparation !== null) ||
    (preparation !== null && preparation.proofSha256 !== proof.proofSha256) ||
    canonicalJsonBytes(value).length > 2 * ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new Error("adjustment shadow metadata custody status differs");
  }
  return Object.freeze(value);
}

// validate one durable metadata consumption checkpoint and response
export function validateAdjustmentShadowMetadataCustodyConsumption(value) {
  requireExactKeys(value, [
    "consumedAt", "contractVersion", "preparationSha256", "proofSha256", "state",
  ], "adjustment shadow metadata custody consumption");
  requireInstant(value.consumedAt, "adjustment shadow metadata consumption time");
  requireSha256(value.preparationSha256,
    "adjustment shadow metadata consumption preparation");
  requireSha256(value.proofSha256, "adjustment shadow metadata consumption proof");
  // admit only one terminal custody-only consumption state
  if (value.contractVersion !== "adjustment-shadow-metadata-custody-consumption/v1" ||
    value.state !== "consumed") {
    throw new TypeError("adjustment shadow metadata custody consumption is invalid");
  }
  return Object.freeze(value);
}

// validate one bounded owner-side compact finalization result
function validateAdjustmentShadowMetadataCustodyResult(value) {
  requireExactKeys(value, [
    "generation", "metadataManifestSha256", "newMetadataRootSha256",
    "registrationSha256", "status",
  ], "adjustment shadow metadata custody result");
  // accept only native success or exact idempotent replay
  if (!Number.isSafeInteger(value.generation) || value.generation < 1 ||
    !["already_finalized", "finalized"].includes(value.status)) {
    throw new TypeError("adjustment shadow metadata custody result is invalid");
  }
  for (const field of [
    "metadataManifestSha256", "newMetadataRootSha256", "registrationSha256",
  ]) {
    requireSha256(value[field], `adjustment shadow metadata custody result ${field}`);
  }
  return Object.freeze(value);
}

// classify bounded backpressure without treating it as a producer gap
function metadataProofPendingError() {
  const error = new Error("adjustment shadow metadata custody proof is pending");
  error.code = "adjustment_shadow_metadata_custody_pending";
  return error;
}

// retain one exact unconsumed metadata proof before hot capsule retirement
export class AdjustmentShadowMetadataCustodyProofStore {
  #now;
  #proofRoot;
  #root;

  // retain only one fixed private proof root and server clock
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#proofRoot = join(this.#root, "shadow-metadata-custody-proofs");
    this.#now = options.now ?? (() => new Date());
  }

  // prepare one proof before publishing its custody acknowledgement
  async prepare(pageValue, acknowledgementValue) {
    await this.#finishConsumption();
    const page = validateAdjustmentRevisionColdPage(pageValue);
    const acknowledgement = validateAdjustmentRevisionColdCustodyAcknowledgementV2(
      acknowledgementValue,
    );
    const expectedMemberRootSha256 = successfulRevisionMemberRoot(
      revisionColdCheckpointEntries(page),
      acknowledgement.startMemberSha256,
    );
    // bind proof preparation to the exact server page and archived member set
    if (acknowledgement.pageSha256 !== page.pageSha256 ||
      acknowledgement.nextArchiveCommitOrdinal !== page.nextArchiveCommitOrdinal ||
      acknowledgement.nextFrontierSha256 !== page.nextFrontierSha256 ||
      acknowledgement.memberRootSha256 !== expectedMemberRootSha256) {
      throw new Error("adjustment shadow metadata custody page differs");
    }
    const entries = page.entries.flatMap((entry) => {
      const parts = revisionColdEntryParts(entry);
      // derive compact metadata only from the distinct shadow capsule grammar
      if (parts.receipts[0].projectionKind !== "shadow_prediction") {
        return [];
      }
      const capsuleBytes = Buffer.from(entry.payload.bytesBase64, "base64");
      const capsule = parseShadowRevisionCapsule(capsuleBytes);
      if (capsule.contractVersion !== "adjustment-shadow-revision-capsule/v2") {
        throw new Error("adjustment shadow metadata custody requires capsule v2");
      }
      const receipt = capsule.revisionReceipt;
      return [{
        archiveCommitOrdinal: receipt.archiveCommitOrdinal,
        archiveCommittedAt: receipt.archiveCommittedAt,
        bodyByteCount: capsule.metadata.bodyByteCount,
        capsuleSha256: sha256(capsuleBytes),
        dueKey: capsule.metadata.dueKey,
        frontierSha256: receipt.frontierSha256,
        inputSha256: capsule.metadata.inputSha256,
        issuedAt: capsule.metadata.issuedAt,
        maxValidAt: capsule.metadata.maxValidAt,
        minValidAt: capsule.metadata.minValidAt,
        predictionBodySha256: capsule.metadata.predictionBodySha256,
        predictionCommittedAt: capsule.predictionCommittedAt,
        predictionSchemaSha256: capsule.metadata.predictionSchemaSha256,
        predictionSha256: capsule.metadata.predictionSha256,
        predecessorFrontierSha256: receipt.predecessorFrontierSha256,
        receiptSha256: receipt.receiptSha256,
        registrationSha256: capsule.metadata.registrationSha256,
        rowCount: capsule.metadata.rowCount,
        sourceReceiptSha256: capsule.metadata.sourceReceiptSha256,
        stageReceiptSha256: receipt.stageReceiptSha256,
      }];
    }).sort((left, right) =>
      left.registrationSha256.localeCompare(right.registrationSha256) ||
        left.predictionSha256.localeCompare(right.predictionSha256));
    await this.#ensureRoot();
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    await this.#readSlot("previous");
    // block every later custody advance until the current DB proof is consumed
    if (current !== null &&
      current.proof.acknowledgementSha256 !== acknowledgement.acknowledgementSha256) {
      throw metadataProofPendingError();
    }
    if (entries.length === 0) {
      if (pending !== null) {
        throw new Error("adjustment shadow metadata custody pending proof differs");
      }
      return null;
    }
    const unsigned = {
      acknowledgementSha256: acknowledgement.acknowledgementSha256,
      archiveCommitOrdinal: acknowledgement.nextArchiveCommitOrdinal,
      authority: "shadow_metadata_custody_only",
      contractVersion: ADJUSTMENT_SHADOW_METADATA_CUSTODY_PROOF_CONTRACT_VERSION,
      custodyCheckpointSha256: acknowledgement.custodyCheckpointSha256,
      entries,
      frontierSha256: acknowledgement.nextFrontierSha256,
      memberRootSha256: acknowledgement.memberRootSha256,
      pageSha256: acknowledgement.pageSha256,
      preparedAt: acknowledgement.acknowledgedAt,
    };
    const proof = validateAdjustmentShadowMetadataCustodyProof({
      ...unsigned,
      proofSha256: sha256(canonicalJsonBytes(unsigned)),
    });
    const bytes = canonicalJsonBytes(proof);
    // converge only the same proof on ACK retry
    if (current !== null) {
      if (!current.bytes.equals(bytes)) {
        throw metadataProofPendingError();
      }
      return proof;
    }
    if (pending !== null) {
      if (!pending.bytes.equals(bytes)) {
        throw new Error("adjustment shadow metadata custody pending proof differs");
      }
    } else {
      await writeExclusive(join(this.#proofRoot, "pending.json"), bytes);
    }
    return proof;
  }

  // publish a prepared proof only after its acknowledgement is durable
  async commit(acknowledgementSha256) {
    requireSha256(acknowledgementSha256,
      "adjustment shadow metadata custody acknowledgement");
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    // converge an already-current proof after a crash
    if (current !== null) {
      if (current.proof.acknowledgementSha256 !== acknowledgementSha256) {
        throw metadataProofPendingError();
      }
      return current.proof;
    }
    if (pending === null) {
      return null;
    }
    if (pending.proof.acknowledgementSha256 !== acknowledgementSha256) {
      throw new Error("adjustment shadow metadata custody acknowledgement differs");
    }
    await writePrivateAtomic(join(this.#proofRoot, "current.json"), pending.bytes);
    await unlink(join(this.#proofRoot, "pending.json"));
    await fsyncDirectory(this.#proofRoot);
    return pending.proof;
  }

  // durably retain exact native arguments before the owner transaction
  async prepareFinalizations(input) {
    await this.#finishConsumption();
    requireExactKeys(input, ["finalizations", "proofSha256"],
      "adjustment shadow metadata preparation input");
    requireSha256(input.proofSha256, "adjustment shadow metadata preparation proof");
    // require one bounded argument per represented registration
    if (!Array.isArray(input.finalizations) || input.finalizations.length < 1 ||
      input.finalizations.length > 2) {
      throw new TypeError("adjustment shadow metadata finalizations are invalid");
    }
    const current = await this.#readSlot("current");
    // prepare only against the exact unconsumed proof
    if (current === null || current.proof.proofSha256 !== input.proofSha256) {
      throw new Error("adjustment shadow metadata custody proof is unavailable");
    }
    const finalizations = input.finalizations.map(
      // validate each owner-computed native function argument
      (value) => validateAdjustmentShadowMetadataFinalization(value),
    ).sort(
      // stabilize the durable registration order
      (left, right) => left.registrationSha256.localeCompare(right.registrationSha256),
    );
    const registrations = [...new Set(current.proof.entries.map(
      // retain every proof registration once
      (entry) => entry.registrationSha256,
    ))].sort();
    // require one and only one finalization for every proof registration
    if (canonicalJson(finalizations.map(
      // compare only the complete prepared registration set
      (value) => value.registrationSha256,
    )) !==
      canonicalJson(registrations)) {
      throw new Error("adjustment shadow metadata finalization set differs");
    }
    // crossbind every range to all and only its exact proof entries
    for (const finalization of finalizations) {
      const entries = current.proof.entries.filter(
        // select one registration's complete proof population
        (entry) => entry.registrationSha256 === finalization.registrationSha256,
      );
      const clocks = entries.map(
        // project the genuine database commit clock retained in each capsule
        (entry) => entry.predictionCommittedAt,
      ).sort();
      if (finalization.coldCommitSha256 !== current.proof.custodyCheckpointSha256 ||
        finalization.maintenanceAnchorSha256 !== current.proof.proofSha256 ||
        finalization.rowCount !== entries.length ||
        finalization.fromCommittedAt !== clocks[0] ||
        finalization.throughCommittedAt !== clocks.at(-1)) {
        throw new Error("adjustment shadow metadata finalization range differs");
      }
    }
    await this.#ensureRoot();
    const existing = await this.#readPreparation("preparation");
    // preserve the first preparation clock on byte-equivalent retries
    if (existing !== null) {
      if (existing.preparation.proofSha256 !== input.proofSha256 ||
        canonicalJson(existing.preparation.finalizations) !== canonicalJson(finalizations)) {
        throw new Error("adjustment shadow metadata custody preparation collision");
      }
      return existing.preparation;
    }
    const unsigned = {
      authority: "shadow_metadata_transfer",
      contractVersion: ADJUSTMENT_SHADOW_METADATA_CUSTODY_PREPARATION_CONTRACT_VERSION,
      finalizations,
      preparedAt: requireNowInstant(this.#now(),
        "adjustment shadow metadata preparation time"),
      proofSha256: input.proofSha256,
    };
    const preparation = validateAdjustmentShadowMetadataCustodyPreparation({
      ...unsigned,
      preparationSha256: sha256(canonicalJsonBytes(unsigned)),
    });
    const bytes = canonicalJsonBytes(preparation);
    try {
      await writeExclusive(join(this.#proofRoot, "preparation.json"), bytes);
    } catch (error) {
      // converge a concurrent byte-identical first preparation
      if (error?.code !== "EEXIST") {
        throw error;
      }
      const raced = await this.#readPreparation("preparation");
      if (raced === null || raced.preparation.proofSha256 !== input.proofSha256 ||
        canonicalJson(raced.preparation.finalizations) !== canonicalJson(finalizations)) {
        throw new Error("adjustment shadow metadata custody preparation collision");
      }
      return raced.preparation;
    }
    return preparation;
  }

  // consume only after owner-side DB finalization covers every registration
  async consume(input) {
    await this.#finishConsumption();
    requireExactKeys(input, ["preparationSha256", "proofSha256", "results"],
      "adjustment shadow metadata custody consumption");
    requireSha256(input.proofSha256, "adjustment shadow metadata custody proof");
    requireSha256(input.preparationSha256,
      "adjustment shadow metadata custody preparation");
    // require one bounded result per prepared registration
    if (!Array.isArray(input.results) || input.results.length < 1 ||
      input.results.length > 2) {
      throw new TypeError("adjustment shadow metadata custody results are invalid");
    }
    const results = input.results.map(
      // validate each owner-returned compact finalization identity
      (result) => validateAdjustmentShadowMetadataCustodyResult(result),
    );
    const priorConsumption = await this.#readConsumption();
    const current = await this.#readSlot("current");
    const preparation = await this.#readPreparation("preparation");
    // converge a retry after the durable consumption checkpoint
    if (current === null && preparation === null && priorConsumption !== null &&
      priorConsumption.proofSha256 === input.proofSha256 &&
      priorConsumption.preparationSha256 === input.preparationSha256) {
      return priorConsumption;
    }
    // reject missing or substituted proof state
    if (current === null || current.proof.proofSha256 !== input.proofSha256) {
      throw new Error("adjustment shadow metadata custody proof is unavailable");
    }
    // reject missing or substituted preparation state
    if (preparation === null ||
      preparation.preparation.preparationSha256 !== input.preparationSha256 ||
      preparation.preparation.proofSha256 !== input.proofSha256) {
      throw new Error("adjustment shadow metadata custody preparation is unavailable");
    }
    const expected = preparation.preparation.finalizations;
    // require native results to match every durable argument identity
    if (results.length !== expected.length || results.some(
      // bind every native result to its durable prepared argument
      (result, index) => result.generation !== expected[index].generation ||
        result.metadataManifestSha256 !== expected[index].metadataManifestSha256 ||
        result.newMetadataRootSha256 !== expected[index].newMetadataRootSha256 ||
        result.registrationSha256 !== expected[index].registrationSha256,
    )) {
      throw new Error("adjustment shadow metadata custody result set differs");
    }
    const consumption = validateAdjustmentShadowMetadataCustodyConsumption({
      consumedAt: requireNowInstant(this.#now(),
        "adjustment shadow metadata custody consumption time"),
      contractVersion: "adjustment-shadow-metadata-custody-consumption/v1",
      preparationSha256: input.preparationSha256,
      proofSha256: input.proofSha256,
      state: "consumed",
    });
    await writePrivateAtomic(join(this.#proofRoot, "previous-preparation.json"),
      preparation.bytes);
    await writePrivateAtomic(join(this.#proofRoot, "previous.json"), current.bytes);
    await writePrivateAtomic(join(this.#proofRoot, "previous-consumption.json"),
      canonicalJsonBytes(consumption));
    await unlink(join(this.#proofRoot, "current.json"));
    await unlink(join(this.#proofRoot, "preparation.json"));
    await fsyncDirectory(this.#proofRoot);
    return consumption;
  }

  // read only the current exact unconsumed proof
  async readCurrent() {
    let details;
    try {
      details = await lstat(this.#proofRoot, { bigint: true });
    } catch (error) {
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    await this.#requireRoot(details);
    const current = await this.#readSlot("current");
    return current === null ? null : Object.freeze({
      proof: current.proof,
      proofSha256: current.proof.proofSha256,
    });
  }

  // read one exact path-free proof and preparation status
  async readStatus() {
    await this.#finishConsumption();
    const current = await this.readCurrent();
    const preparation = await this.#readPreparation("preparation");
    return validateAdjustmentShadowMetadataCustodyStatus({
      contractVersion: ADJUSTMENT_SHADOW_METADATA_CUSTODY_STATUS_CONTRACT_VERSION,
      preparation: preparation?.preparation ?? null,
      proof: current?.proof ?? null,
    });
  }

  // read only the last exact consumption receipt for one proof identity
  async readConsumed(proofSha256) {
    requireSha256(proofSha256, "adjustment shadow metadata consumption proof");
    await this.#finishConsumption();
    const consumption = await this.#readConsumption();
    // distinguish a genuine empty bounded history
    if (consumption === null) {
      return null;
    }
    // prohibit enumeration or adoption of another proof receipt
    if (consumption.proofSha256 !== proofSha256) {
      throw new Error("adjustment shadow metadata consumption proof differs");
    }
    return consumption;
  }

  // create only the fixed private proof directory
  async #ensureRoot() {
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(this.#proofRoot);
    await this.#requireRoot(await lstat(this.#proofRoot, { bigint: true }));
  }

  // read one exact bounded proof slot
  async #readSlot(slot) {
    const bytes = await readOptionalPrivateFile(
      join(this.#proofRoot, `${slot}.json`),
      ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES,
    );
    if (bytes === null) {
      return null;
    }
    const proof = validateAdjustmentShadowMetadataCustodyProof(
      JSON.parse(bytes.toString("utf8")),
    );
    if (!bytes.equals(canonicalJsonBytes(proof))) {
      throw new Error("adjustment shadow metadata custody proof is not canonical");
    }
    return { bytes, proof };
  }

  // read one canonical bounded finalization preparation slot
  async #readPreparation(slot) {
    const bytes = await readOptionalPrivateFile(
      join(this.#proofRoot, `${slot}.json`),
      ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES,
    );
    if (bytes === null) {
      return null;
    }
    const preparation = validateAdjustmentShadowMetadataCustodyPreparation(
      JSON.parse(bytes.toString("utf8")),
    );
    if (!bytes.equals(canonicalJsonBytes(preparation))) {
      throw new Error("adjustment shadow metadata custody preparation is not canonical");
    }
    return { bytes, preparation };
  }

  // read the last durable consumption checkpoint when present
  async #readConsumption() {
    const bytes = await readOptionalPrivateFile(
      join(this.#proofRoot, "previous-consumption.json"),
      ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES,
    );
    if (bytes === null) {
      return null;
    }
    const consumption = validateAdjustmentShadowMetadataCustodyConsumption(
      JSON.parse(bytes.toString("utf8")),
    );
    if (!bytes.equals(canonicalJsonBytes(consumption))) {
      throw new Error("adjustment shadow metadata consumption is not canonical");
    }
    return consumption;
  }

  // finish cleanup only after a matching durable consumption checkpoint
  async #finishConsumption() {
    const consumption = await this.#readConsumption();
    if (consumption === null) {
      return;
    }
    const current = await this.#readSlot("current");
    const preparation = await this.#readPreparation("preparation");
    // leave a later proof untouched when the checkpoint belongs to its predecessor
    if (current !== null && current.proof.proofSha256 !== consumption.proofSha256) {
      return;
    }
    if (preparation !== null &&
      preparation.preparation.preparationSha256 !== consumption.preparationSha256) {
      return;
    }
    const previous = await this.#readSlot("previous");
    const previousPreparation = await this.#readPreparation("previous-preparation");
    // require exact redundant bytes before completing any interrupted unlink
    if (previous === null || previousPreparation === null ||
      previous.proof.proofSha256 !== consumption.proofSha256 ||
      previousPreparation.preparation.preparationSha256 !==
        consumption.preparationSha256) {
      throw new Error("adjustment shadow metadata consumption checkpoint differs");
    }
    if (current !== null) {
      await unlink(join(this.#proofRoot, "current.json"));
    }
    if (preparation !== null) {
      await unlink(join(this.#proofRoot, "preparation.json"));
    }
    await fsyncDirectory(this.#proofRoot);
  }

  // require one exact owner-private proof directory
  async #requireRoot(details) {
    if (!details.isDirectory() || details.isSymbolicLink() ||
      details.uid !== BigInt(process.getuid()) || details.gid !== BigInt(process.getgid()) ||
      (details.mode & 0o777n) !== 0o700n ||
      await realpath(this.#proofRoot) !== this.#proofRoot) {
      throw new Error("adjustment shadow metadata custody root is unsafe");
    }
  }
}

// validate one closed custody request without trusting its checkpoint identity
function validateAdjustmentRevisionCustodyRequest(input, identityField) {
  requireExactKeys(input, [
    "afterArchiveCommitOrdinal", "afterFrontierSha256", identityField,
    "memberRootSha256", "pageSha256", "previousPageSha256", "root",
    "startMemberSha256", "startSha256", "watermarkArchiveCommitOrdinal",
    "watermarkFrontierSha256",
  ], "adjustment revision custody acknowledgement request");
  // validate both server-produced cursor ordinals
  for (const field of ["afterArchiveCommitOrdinal", "watermarkArchiveCommitOrdinal"]) {
    requireUint64Text(input[field], `adjustment revision custody ${field}`);
  }
  // validate every required content identity
  for (const field of [
    "afterFrontierSha256", identityField, "memberRootSha256",
    "pageSha256", "previousPageSha256", "startSha256", "watermarkFrontierSha256",
  ]) {
    requireSha256(input[field], `adjustment revision custody ${field}`);
  }
  // accept a start member only for the first page of one frozen transfer
  if (input.startMemberSha256 !== null) {
    requireSha256(input.startMemberSha256,
      "adjustment revision custody start member");
  }
  requireRevisionArchiveRoot(input.root);
}

// persist one custody-only checkpoint before retiring its exact hot page
async function acknowledgeAdjustmentRevisionColdCustody(input, options, contract) {
  const root = requireRevisionArchiveRoot(input.root);
  await requireRevisionCustodyDirectories(root);
  return await withRevisionSpoolLock(root, async () => {
    await requireSuccessfulRevisionSpoolCensus(root);
    const state = await readRevisionCustodyAcknowledgements(root);
    const current = state.current;
    const metadataProofStore = contract.contractVersion ===
        ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION
      ? new AdjustmentShadowMetadataCustodyProofStore({ root })
      : null;

    // converge a retry only on the exact first durable custody identity
    if (current?.pageSha256 === input.pageSha256) {
      requireAdjustmentRevisionCustodyRetry(current, input, contract);
      // finish a proof commit that may have crashed after durable ACK publication
      if (metadataProofStore !== null) {
        await metadataProofStore.commit(current.acknowledgementSha256);
      }
      await retireAdjustmentRevisionCustodyEntries(root, current.retirementEntries);
      return current;
    }
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal: input.afterArchiveCommitOrdinal,
      afterFrontierSha256: input.afterFrontierSha256,
      previousPageSha256: input.previousPageSha256,
      root,
      startSha256: input.startSha256,
      watermarkArchiveCommitOrdinal: input.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: input.watermarkFrontierSha256,
    });
    // retire only a nonempty page actually observed and archived by the caller
    if (page.pageSha256 !== input.pageSha256 || page.entries.length === 0) {
      throw new Error("adjustment revision custody page differs");
    }
    const firstPage = page.previousPageSha256 === page.startSha256;
    // bind the distinct first-page start member without accepting one later
    if (firstPage !== (input.startMemberSha256 !== null)) {
      throw new Error("adjustment revision custody start member differs");
    }
    const checkpointEntries = revisionColdCheckpointEntries(page);
    const memberRootSha256 = successfulRevisionMemberRoot(
      checkpointEntries,
      input.startMemberSha256,
    );
    // require the actual archived member population before deleting redundancy
    if (memberRootSha256 !== input.memberRootSha256) {
      throw new Error("adjustment revision custody member root differs");
    }
    const expectedAfterOrdinal = current?.nextArchiveCommitOrdinal ?? "0";
    const expectedAfterFrontier = current?.nextFrontierSha256 ??
      adjustmentRevisionGenesisFrontier();
    // advance only the direct acknowledged cursor successor
    if (page.afterArchiveCommitOrdinal !== expectedAfterOrdinal ||
      page.afterFrontierSha256 !== expectedAfterFrontier ||
      (current !== null && page.startSha256 === current.startSha256 &&
        page.previousPageSha256 !== current.pageSha256)) {
      throw new Error("adjustment revision custody cursor differs");
    }
    const retirementEntries = adjustmentRevisionCustodyRetirementEntries(page);
    const unsigned = {
      acknowledgedAt: requireNowInstant(
        (options.now ?? (() => new Date()))(),
        "adjustment revision custody time",
      ),
      afterArchiveCommitOrdinal: page.afterArchiveCommitOrdinal,
      afterFrontierSha256: page.afterFrontierSha256,
      authority: "cold_custody_only",
      contractVersion: contract.contractVersion,
      [contract.identityField]: input[contract.identityField],
      memberRootSha256,
      nextArchiveCommitOrdinal: page.nextArchiveCommitOrdinal,
      nextFrontierSha256: page.nextFrontierSha256,
      pageSha256: page.pageSha256,
      previousAcknowledgementSha256: current?.acknowledgementSha256 ?? null,
      previousPageSha256: page.previousPageSha256,
      retirementEntries,
      startMemberSha256: input.startMemberSha256,
      startSha256: page.startSha256,
      watermarkArchiveCommitOrdinal: page.watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256: page.watermarkFrontierSha256,
    };
    const acknowledgement = contract.validate({
      ...unsigned,
      acknowledgementSha256: sha256(canonicalJsonBytes(unsigned)),
    });
    // persist capsule-derived metadata proof before ACK publication or unlink
    if (metadataProofStore !== null) {
      await metadataProofStore.prepare(page, acknowledgement);
    }
    const directory = join(root, "revision-custody-acknowledgements");
    // retain exactly one prior checkpoint before publishing the next current state
    if (state.currentBytes !== null) {
      await writeAtomicPrivate(join(directory, "previous.json"), state.currentBytes);
    }
    await writeAtomicPrivate(join(directory, "current.json"),
      canonicalJsonBytes(acknowledgement));
    // expose proof current only after the exact ACK is itself durable
    if (metadataProofStore !== null) {
      await metadataProofStore.commit(acknowledgement.acknowledgementSha256);
    }
    // expose a deterministic crash seam only after the durable custody checkpoint
    if (typeof options.afterDurableAcknowledgement === "function") {
      await options.afterDurableAcknowledgement(acknowledgement);
    }
    await retireAdjustmentRevisionCustodyEntries(root, retirementEntries);
    return acknowledgement;
  });
}

// validate one bounded custody-only acknowledgement and its retirement set
export function validateAdjustmentRevisionColdCustodyAcknowledgement(value) {
  return validateAdjustmentRevisionColdCustodyAcknowledgementContract(value, {
    contractVersion: ADJUSTMENT_REVISION_CUSTODY_ACK_CONTRACT_VERSION,
    identityField: "graphManifestSha256",
  });
}

// validate one bounded packed-custody acknowledgement and its retirement set
export function validateAdjustmentRevisionColdCustodyAcknowledgementV2(value) {
  return validateAdjustmentRevisionColdCustodyAcknowledgementContract(value, {
    contractVersion: ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION,
    identityField: "custodyCheckpointSha256",
  });
}

// validate one versioned custody-only acknowledgement contract
function validateAdjustmentRevisionColdCustodyAcknowledgementContract(value, contract) {
  requireExactKeys(value, [
    "acknowledgedAt", "acknowledgementSha256", "afterArchiveCommitOrdinal",
    "afterFrontierSha256", "authority", "contractVersion", contract.identityField,
    "memberRootSha256", "nextArchiveCommitOrdinal", "nextFrontierSha256",
    "pageSha256", "previousAcknowledgementSha256", "previousPageSha256",
    "retirementEntries", "startMemberSha256", "startSha256",
    "watermarkArchiveCommitOrdinal", "watermarkFrontierSha256",
  ], "adjustment revision custody acknowledgement");
  requireInstant(value.acknowledgedAt, "adjustment revision custody acknowledgedAt");
  // validate every bounded ordinal independently
  for (const field of [
    "afterArchiveCommitOrdinal", "nextArchiveCommitOrdinal",
    "watermarkArchiveCommitOrdinal",
  ]) {
    requireUint64Text(value[field], `adjustment revision custody ${field}`);
  }
  // validate all nonnullable content identities
  for (const field of [
    "acknowledgementSha256", "afterFrontierSha256", contract.identityField,
    "memberRootSha256", "nextFrontierSha256", "pageSha256", "previousPageSha256",
    "startSha256", "watermarkFrontierSha256",
  ]) {
    requireSha256(value[field], `adjustment revision custody ${field}`);
  }
  // accept nullable predecessor and start identities only at their fixed boundaries
  for (const field of ["previousAcknowledgementSha256", "startMemberSha256"]) {
    // validate only identities that exist at this boundary
    if (value[field] !== null) {
      requireSha256(value[field], `adjustment revision custody ${field}`);
    }
  }
  // prohibit any semantic authority or unbounded retirement set
  if (value.authority !== "cold_custody_only" ||
    value.contractVersion !== contract.contractVersion ||
    !Array.isArray(value.retirementEntries) || value.retirementEntries.length < 4 ||
    value.retirementEntries.length > 10 ||
    BigInt(value.nextArchiveCommitOrdinal) <= BigInt(value.afterArchiveCommitOrdinal)) {
    throw new TypeError("adjustment revision custody acknowledgement is invalid");
  }
  const retirementEntries = value.retirementEntries.map(
    // validate only exact file identities made redundant by this cold page
    (entry) => validateAdjustmentRevisionCustodyRetirementEntry(entry),
  );
  // require a stable exact retirement order and one canonical acknowledgement
  if (canonicalJson(retirementEntries) !==
      canonicalJson([...retirementEntries].sort(compareRevisionCustodyRetirementEntries)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_REVISION_CUSTODY_ACK_MAXIMUM_BYTES) {
    throw new Error("adjustment revision custody retirement set differs");
  }
  const unsigned = { ...value };
  delete unsigned.acknowledgementSha256;
  // bind all checkpoint and retirement fields without granting qualification authority
  if (value.acknowledgementSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment revision custody acknowledgement identity differs");
  }
  return Object.freeze(value);
}

// derive the exact redundant hot files represented by one validated cold page
function adjustmentRevisionCustodyRetirementEntries(pageValue) {
  const page = validateAdjustmentRevisionColdPage(pageValue);
  const entries = [];
  // append one exact addressed hot representation
  const add = (kind, identitySha256, fileSha256) => {
    entries.push({ fileSha256, identitySha256, kind });
  };
  // bind every receipt and its complete auxiliary representations
  for (const entry of page.entries) {
    const parts = revisionColdEntryParts(entry);
    const first = parts.receipts[0];
    // retire a grouped chain only as its one immutable all-receipt file
    if (parts.batch) {
      const group = {
        contractVersion: ADJUSTMENT_REVISION_BATCH_COMMIT_GROUP_CONTRACT_VERSION,
        projectionIdentitySha256: first.projectionIdentitySha256,
        receipts: parts.receipts,
        successors: parts.successors,
      };
      validateRevisionBatchCommitGroup(group);
      add("revision_commit_group", first.projectionIdentitySha256,
        sha256(canonicalJsonBytes(group)));
    } else {
      add("revision_commit_receipt", first.frontierSha256,
        sha256(canonicalJsonBytes(first)));
      add("revision_frontier_successor", first.predecessorFrontierSha256,
        sha256(canonicalJsonBytes(parts.successors[0])));
    }
    if (first.projectionKind === "shadow_prediction") {
      const capsule = parseShadowRevisionCapsule(
        Buffer.from(entry.payload.bytesBase64, "base64"),
      );
      add("shadow_revision_capsule", first.receiptSha256,
        entry.payload.identitySha256);
      if (entry.publication.disposition === "published") {
        add("shadow_revision_publish_receipt", capsule.metadata.predictionSha256,
          sha256(canonicalJsonBytes(entry.publication.value)));
      } else {
        const gapIdentitySha256 = sha256(canonicalJsonBytes({
          dueKey: entry.publication.value.input.dueKey,
          registrationSha256: entry.publication.value.input.registrationSha256,
        }));
        add("shadow_revision_terminal_gap", gapIdentitySha256,
          sha256(canonicalJsonBytes(entry.publication.value)));
      }
    } else {
      add("revision_projection", first.projectionIdentitySha256,
        entry.payload.identitySha256);
      add("revision_stage_receipt", first.projectionIdentitySha256,
        sha256(canonicalJsonBytes(entry.stageReceipt)));
      // retire public control state only after the same exact cold-page custody
      if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
        add("rain_control_state", entry.rainControlState.stateSha256,
          entry.rainControlState.identitySha256);
        add("rain_control_state_stage_receipt", entry.rainControlState.stateSha256,
          sha256(canonicalJsonBytes(entry.rainControlStateStageReceipt)));
      }
      if (entry.publication.disposition === "published") {
        add("revision_publish_receipt", first.projectionIdentitySha256,
          sha256(canonicalJsonBytes(entry.publication.value)));
      } else {
        const gapIdentitySha256 = sha256(canonicalJsonBytes({
          logicalKeySha256: entry.publication.value.input.logicalKeySha256,
          projectionIdentitySha256:
            entry.publication.value.input.projectionIdentitySha256,
        }));
        add("revision_terminal_gap", gapIdentitySha256,
          sha256(canonicalJsonBytes(entry.publication.value)));
      }
    }
  }
  return entries.sort(compareRevisionCustodyRetirementEntries);
}

// validate one fixed successful-spool retirement address
function validateAdjustmentRevisionCustodyRetirementEntry(value) {
  requireExactKeys(value, ["fileSha256", "identitySha256", "kind"],
    "adjustment revision custody retirement entry");
  for (const field of ["fileSha256", "identitySha256"]) {
    requireSha256(value[field], `adjustment revision custody retirement ${field}`);
  }
  if (!["revision_commit_receipt", "revision_frontier_successor",
    "revision_commit_group", "revision_projection", "revision_publish_receipt",
    "revision_stage_receipt",
    "rain_control_state", "rain_control_state_stage_receipt",
    "revision_terminal_gap", "shadow_revision_capsule",
    "shadow_revision_publish_receipt", "shadow_revision_terminal_gap"]
    .includes(value.kind)) {
    throw new TypeError("adjustment revision custody retirement kind is invalid");
  }
  return value;
}

// compare one fixed retirement file address
function compareRevisionCustodyRetirementEntries(left, right) {
  return left.kind.localeCompare(right.kind, "en") ||
    left.identitySha256.localeCompare(right.identitySha256, "en");
}

// bind one acknowledgement retry to every original caller identity
function requireAdjustmentRevisionCustodyRetry(current, input, contract) {
  const fields = [
    "afterArchiveCommitOrdinal", "afterFrontierSha256", contract.identityField,
    "memberRootSha256", "pageSha256", "previousPageSha256", "startMemberSha256",
    "startSha256", "watermarkArchiveCommitOrdinal", "watermarkFrontierSha256",
  ];
  // reject any relabelling of the first durable custody checkpoint
  if (current.contractVersion !== contract.contractVersion ||
    fields.some((field) => current[field] !== input[field])) {
    throw new Error("adjustment revision custody acknowledgement collision");
  }
}

// read one fixed unqualified-payload frontier without assigning an archive ordinal
export async function readAdjustmentRevisionGapTransferStart(options = {}) {
  const root = requireRevisionArchiveRoot(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
  await requireRevisionGapTransferDirectories(root);
  const frontier = await readRevisionGapFrontier(root);
  const unsigned = {
    contractVersion: ADJUSTMENT_REVISION_GAP_START_CONTRACT_VERSION,
    frontierSha256: frontier.frontierSha256,
    pageCount: frontier.pageCount,
  };
  return validateAdjustmentRevisionGapTransferStart({
    ...unsigned,
    startSha256: sha256(canonicalJsonBytes(unsigned)),
  });
}

// freeze one bounded page of durable gaps and their exact staged bytes
export async function readAdjustmentRevisionGapPayloadPage(input) {
  requireExactKeys(input, ["frontierSha256", "root", "startSha256"],
    "adjustment revision gap page request");
  requireSha256(input.frontierSha256, "adjustment revision gap request frontier");
  requireSha256(input.startSha256, "adjustment revision gap request start");
  const root = requireRevisionArchiveRoot(input.root);
  await requireRevisionGapTransferDirectories(root);
  return await withRevisionSpoolLock(root, async () => {
    const start = await readAdjustmentRevisionGapTransferStart({ root });
    // bind the page to the exact workstation-observed gap frontier
    if (start.startSha256 !== input.startSha256 ||
      start.frontierSha256 !== input.frontierSha256) {
      throw new Error("adjustment revision gap transfer start differs");
    }
    const pendingPath = join(root, "revision-gap-transfer", "pending.json");
    const pendingBytes = await readOptionalPrivateFile(
      pendingPath,
      ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES,
    );

    // return one immutable pending page until its exact graph is acknowledged
    if (pendingBytes !== null) {
      const pending = validateAdjustmentRevisionGapPayloadPage(
        JSON.parse(pendingBytes.toString("utf8")),
      );
      if (!pendingBytes.equals(canonicalJsonBytes(pending)) ||
        pending.startSha256 !== input.startSha256 ||
        pending.predecessorFrontierSha256 !== input.frontierSha256) {
        throw new Error("adjustment revision gap pending page differs");
      }
      return pending;
    }
    const available = await collectAdjustmentRevisionGapEntries(root);
    let entries = [];
    // retain at most two entries and stop before the exact wire ceiling
    for (const entry of available) {
      const candidate = buildAdjustmentRevisionGapPayloadPage({
        entries: [...entries, entry],
        predecessorFrontierSha256: input.frontierSha256,
        startSha256: input.startSha256,
      });
    if (canonicalJsonBytes(candidate).length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES) {
        // reject even one payload that cannot fit the reviewed wire bound
        if (entries.length === 0) {
          throw new RangeError("adjustment revision gap payload exceeds page limit");
        }
        break;
      }
      entries = candidate.entries;
      // stop at the reviewed online payload slot count
      if (entries.length === ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS) {
        break;
      }
    }
    const page = buildAdjustmentRevisionGapPayloadPage({
      entries,
      predecessorFrontierSha256: input.frontierSha256,
      startSha256: input.startSha256,
    });
    validateAdjustmentRevisionGapPayloadPage(page);
    // freeze only value-bearing pages that require an acknowledgement
    if (!page.idle) {
      await writeExclusive(pendingPath, canonicalJsonBytes(page));
    }
    return Object.freeze(page);
  });
}

// acknowledge one verified cold graph and release only its exact unqualified slots
export async function acknowledgeAdjustmentRevisionGapPayload(input) {
  requireExactKeys(input, ["graphManifestSha256", "pageSha256", "root"],
    "adjustment revision gap acknowledgement request");
  requireSha256(input.graphManifestSha256,
    "adjustment revision gap acknowledgement graph");
  requireSha256(input.pageSha256, "adjustment revision gap acknowledgement page");
  const root = requireRevisionArchiveRoot(input.root);
  await requireRevisionGapTransferDirectories(root);
  return await withRevisionSpoolLock(root, async () => {
    const frontier = await readRevisionGapFrontier(root);
    const pendingPath = join(root, "revision-gap-transfer", "pending.json");
    const pendingBytes = await readOptionalPrivateFile(
      pendingPath,
      ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES,
    );
    // converge only the latest byte-identical graph acknowledgement
    if (frontier.pageSha256 === input.pageSha256) {
      // preserve the first acknowledged cold graph identity
      if (frontier.graphManifestSha256 !== input.graphManifestSha256) {
        throw new Error("adjustment revision gap acknowledgement collision");
      }
      // finish only crash-interrupted deletion of the same pending page
      if (pendingBytes !== null) {
        const pending = validateAdjustmentRevisionGapPayloadPage(
          JSON.parse(pendingBytes.toString("utf8")),
        );
        await retireAcknowledgedRevisionGapSlots(root, pending);
        await unlink(pendingPath).catch(
          // accept only a retry after the pending page was already removed
          (error) => error?.code === "ENOENT" ? undefined : Promise.reject(error),
        );
        await fsyncDirectory(dirname(pendingPath));
      }
      return frontier;
    }
    // require the exact frozen nonempty page before advancing the gap frontier
    if (pendingBytes === null) {
      throw new Error("adjustment revision gap pending page is unavailable");
    }
    const page = validateAdjustmentRevisionGapPayloadPage(
      JSON.parse(pendingBytes.toString("utf8")),
    );
    if (!pendingBytes.equals(canonicalJsonBytes(page)) || page.idle ||
      page.pageSha256 !== input.pageSha256 ||
      page.predecessorFrontierSha256 !== frontier.frontierSha256) {
      throw new Error("adjustment revision gap acknowledgement page differs");
    }
    const acknowledgedAt = new Date().toISOString();
    const pageCount = (BigInt(frontier.pageCount) + 1n).toString();
    const nextFrontierSha256 = sha256(Buffer.from(
      `${ADJUSTMENT_REVISION_GAP_ACK_CONTRACT_VERSION}\n${frontier.frontierSha256}\n` +
      `${page.pageSha256}\n${input.graphManifestSha256}\n${page.memberRootSha256}`,
    ));
    const unsigned = {
      acknowledgedAt,
      contractVersion: ADJUSTMENT_REVISION_GAP_ACK_CONTRACT_VERSION,
      frontierSha256: nextFrontierSha256,
      graphManifestSha256: input.graphManifestSha256,
      memberRootSha256: page.memberRootSha256,
      pageCount,
      pageSha256: page.pageSha256,
      predecessorFrontierSha256: frontier.frontierSha256,
    };
    const acknowledgement = {
      ...unsigned,
      acknowledgementSha256: sha256(canonicalJsonBytes(unsigned)),
    };
    validateAdjustmentRevisionGapPayloadAcknowledgement(acknowledgement);
    await writeAtomicPrivate(
      join(root, "revision-gap-transfer", "current.json"),
      canonicalJsonBytes(acknowledgement),
    );
    await retireAcknowledgedRevisionGapSlots(root, page);
    await unlink(pendingPath);
    await fsyncDirectory(dirname(pendingPath));
    return Object.freeze(acknowledgement);
  });
}

// validate one bounded non-ordinal gap payload page and its exact member root
export function validateAdjustmentRevisionGapPayloadPage(value) {
  requireExactKeys(value, [
    "contractVersion", "entries", "idle", "memberRootSha256", "pageSha256",
    "predecessorFrontierSha256", "startSha256",
  ], "adjustment revision gap payload page");
  // require only a current two-slot page or one explicit idle response
  if (value.contractVersion !== ADJUSTMENT_REVISION_GAP_PAGE_CONTRACT_VERSION ||
    !Array.isArray(value.entries) ||
    value.entries.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    typeof value.idle !== "boolean" || value.idle !== (value.entries.length === 0)) {
    throw new TypeError("adjustment revision gap payload page is invalid");
  }
  for (const field of [
    "memberRootSha256", "pageSha256", "predecessorFrontierSha256", "startSha256",
  ]) {
    requireSha256(value[field], `adjustment revision gap payload page ${field}`);
  }
  const normalized = value.entries.map(
    // validate every actual retained staged representation
    (entry) => validateAdjustmentRevisionGapEntry(entry),
  );
  const memberRootSha256 = revisionGapMemberRoot(normalized);
  const unsigned = {
    contractVersion: value.contractVersion,
    entries: normalized,
    idle: value.idle,
    memberRootSha256,
    predecessorFrontierSha256: value.predecessorFrontierSha256,
    startSha256: value.startSha256,
  };
  // bind exact page bytes without granting any successful-receipt semantics
  if (value.memberRootSha256 !== memberRootSha256 ||
    value.pageSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    canonicalJsonBytes(value).length > ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES) {
    throw new Error("adjustment revision gap payload page identity differs");
  }
  return Object.freeze(value);
}

// validate one compact non-ordinal gap transfer start
export function validateAdjustmentRevisionGapTransferStart(value) {
  requireExactKeys(value, [
    "contractVersion", "frontierSha256", "pageCount", "startSha256",
  ], "adjustment revision gap transfer start");
  requireSha256(value.frontierSha256, "adjustment revision gap start frontier");
  requireSha256(value.startSha256, "adjustment revision gap start identity");
  requireUint64Text(value.pageCount, "adjustment revision gap start page count");
  const unsigned = {
    contractVersion: value.contractVersion,
    frontierSha256: value.frontierSha256,
    pageCount: value.pageCount,
  };
  // bind the caller cursor to one exact compact frontier projection
  if (value.contractVersion !== ADJUSTMENT_REVISION_GAP_START_CONTRACT_VERSION ||
    value.startSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment revision gap transfer start differs");
  }
  return Object.freeze(value);
}

// validate one server-durable unqualified gap graph acknowledgement
export function validateAdjustmentRevisionGapPayloadAcknowledgement(value) {
  requireExactKeys(value, [
    "acknowledgedAt", "acknowledgementSha256", "contractVersion", "frontierSha256",
    "graphManifestSha256", "memberRootSha256", "pageCount", "pageSha256",
    "predecessorFrontierSha256",
  ], "adjustment revision gap acknowledgement");
  requireInstant(value.acknowledgedAt, "adjustment revision gap acknowledgedAt");
  requireUint64Text(value.pageCount, "adjustment revision gap page count");
  for (const field of [
    "acknowledgementSha256", "frontierSha256", "graphManifestSha256",
    "memberRootSha256", "pageSha256", "predecessorFrontierSha256",
  ]) {
    requireSha256(value[field], `adjustment revision gap acknowledgement ${field}`);
  }
  const unsigned = { ...value };
  delete unsigned.acknowledgementSha256;
  const frontierSha256 = sha256(Buffer.from(
    `${value.contractVersion}\n${value.predecessorFrontierSha256}\n` +
    `${value.pageSha256}\n${value.graphManifestSha256}\n${value.memberRootSha256}`,
  ));
  // bind the exact page, cold graph and actual member root to one server checkpoint
  if (value.contractVersion !== ADJUSTMENT_REVISION_GAP_ACK_CONTRACT_VERSION ||
    value.pageCount === "0" || value.frontierSha256 !== frontierSha256 ||
    value.acknowledgementSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment revision gap acknowledgement differs");
  }
  return Object.freeze(value);
}

// project one gap page into nonduplicating plaintext-archive graph members
export function buildAdjustmentRevisionGapGraphSegment(pageValue) {
  const page = validateAdjustmentRevisionGapPayloadPage(pageValue);
  // never acknowledge a graph for an idle transfer response
  if (page.idle) {
    throw new TypeError("adjustment revision idle gap page has no graph segment");
  }
  const checkpoint = {
    contractVersion: ADJUSTMENT_REVISION_GAP_CHECKPOINT_CONTRACT_VERSION,
    entries: page.entries.map(
      // retain only graph identities and exact member hashes, never duplicate payload bytes
      (entry) => ({
        gapMemberSha256: entry.gapMemberSha256,
        gapSha256: entry.gap.gapSha256,
        payloadIdentitySha256: entry.payload.identitySha256,
        payloadKind: entry.payload.kind,
        ...(entry.payload.kind === "adjustment-rain-gate-control-projection/v3"
          ? {
              rainControlStateIdentitySha256: entry.rainControlState.identitySha256,
              rainControlStateStageReceiptMemberSha256:
                entry.rainControlStateStageReceiptMemberSha256,
              rainControlStateStageReceiptSha256:
                entry.rainControlStateStageReceipt.stageReceiptSha256,
            }
          : {}),
        slotIdentitySha256: entry.slotIdentitySha256,
        stageReceiptMemberSha256: entry.stageReceiptMemberSha256,
        stageReceiptSha256: entry.stageReceipt.stageReceiptSha256,
      }),
    ),
    memberRootSha256: page.memberRootSha256,
    pageSha256: page.pageSha256,
    predecessorFrontierSha256: page.predecessorFrontierSha256,
    startSha256: page.startSha256,
  };
  validateAdjustmentRevisionGapCheckpoint(checkpoint);
  const members = [{
    identitySha256: page.pageSha256,
    kind: ADJUSTMENT_REVISION_GAP_CHECKPOINT_CONTRACT_VERSION,
    payload: canonicalJsonBytes(checkpoint),
  }];
  const crossLinks = [];
  // include every exact gap, value and stage receipt as distinct cold members
  for (const entry of page.entries) {
    members.push(
      {
        identitySha256: entry.gap.gapSha256,
        kind: entry.gap.contractVersion,
        payload: canonicalJsonBytes(entry.gap),
      },
      {
        identitySha256: entry.payload.identitySha256,
        kind: entry.payload.kind,
        payload: Buffer.from(entry.payload.bytesBase64, "base64"),
      },
      {
        identitySha256: entry.stageReceipt.stageReceiptSha256,
        kind: entry.stageReceipt.contractVersion,
        payload: canonicalJsonBytes(entry.stageReceipt),
      },
    );
    // retain the actual monthly state and its first durable availability proof
    if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
      members.push(
        {
          identitySha256: entry.rainControlState.identitySha256,
          kind: entry.rainControlState.kind,
          payload: Buffer.from(entry.rainControlState.bytesBase64, "base64"),
        },
        {
          identitySha256: entry.rainControlStateStageReceipt.stageReceiptSha256,
          kind: entry.rainControlStateStageReceipt.contractVersion,
          payload: canonicalJsonBytes(entry.rainControlStateStageReceipt),
        },
      );
    }
    crossLinks.push(
      {
        fromIdentitySha256: page.pageSha256,
        relation: "contains_gap",
        toIdentitySha256: entry.gap.gapSha256,
      },
      {
        fromIdentitySha256: entry.gap.gapSha256,
        relation: "binds_unqualified_payload",
        toIdentitySha256: entry.payload.identitySha256,
      },
      {
        fromIdentitySha256: entry.gap.gapSha256,
        relation: "binds_stage_receipt",
        toIdentitySha256: entry.stageReceipt.stageReceiptSha256,
      },
    );
    // bind the unqualified body to the exact archived control-state auxiliaries
    if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
      crossLinks.push(
        {
          fromIdentitySha256: entry.gap.gapSha256,
          relation: "binds_rain_control_state",
          toIdentitySha256: entry.rainControlState.identitySha256,
        },
        {
          fromIdentitySha256: entry.gap.gapSha256,
          relation: "binds_rain_control_state_stage_receipt",
          toIdentitySha256:
            entry.rainControlStateStageReceipt.stageReceiptSha256,
        },
      );
    }
  }
  return Object.freeze({ crossLinks: Object.freeze(crossLinks),
    members: Object.freeze(members) });
}

// validate one compact page checkpoint without accepting hash-only payload availability
export function validateAdjustmentRevisionGapCheckpoint(value) {
  requireExactKeys(value, [
    "contractVersion", "entries", "memberRootSha256", "pageSha256",
    "predecessorFrontierSha256", "startSha256",
  ], "adjustment revision gap checkpoint");
  // require one nonempty bounded checkpoint
  if (value.contractVersion !== ADJUSTMENT_REVISION_GAP_CHECKPOINT_CONTRACT_VERSION ||
    !Array.isArray(value.entries) || value.entries.length < 1 ||
    value.entries.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS) {
    throw new TypeError("adjustment revision gap checkpoint is invalid");
  }
  for (const field of [
    "memberRootSha256", "pageSha256", "predecessorFrontierSha256", "startSha256",
  ]) {
    requireSha256(value[field], `adjustment revision gap checkpoint ${field}`);
  }
  // validate every member address retained by the compact checkpoint
  for (const entry of value.entries) {
    const rainControl = entry.payloadKind ===
      "adjustment-rain-gate-control-projection/v3";
    requireExactKeys(entry, [
      "gapMemberSha256", "gapSha256", "payloadIdentitySha256", "payloadKind",
      ...(rainControl ? ["rainControlStateIdentitySha256",
        "rainControlStateStageReceiptMemberSha256",
        "rainControlStateStageReceiptSha256"] : []),
      "slotIdentitySha256", "stageReceiptMemberSha256", "stageReceiptSha256",
    ], "adjustment revision gap checkpoint entry");
    for (const field of [
      "gapMemberSha256", "gapSha256", "payloadIdentitySha256", "slotIdentitySha256",
      "stageReceiptMemberSha256", "stageReceiptSha256",
      ...(rainControl ? ["rainControlStateIdentitySha256",
        "rainControlStateStageReceiptMemberSha256",
        "rainControlStateStageReceiptSha256"] : []),
    ]) {
      requireSha256(entry[field], `adjustment revision gap checkpoint ${field}`);
    }
    if (!["adjustment-revision-projection/v1", "adjustment-revision-batch-projection/v2",
      "adjustment-rain-gate-feature-projection/v2",
      "adjustment-rain-gate-control-projection/v3",
      "adjustment-rain-fixed-gauge-target-projection/v1",
      "adjustment-temperature-native-source-projection/v2",
      "rain-maintenance-control-state/v1",
      "adjustment-shadow-revision-stage/v1", "adjustment-shadow-revision-stage/v2"]
      .includes(entry.payloadKind)) {
      throw new TypeError("adjustment revision gap checkpoint payload kind is invalid");
    }
  }
  const expectedMemberRootSha256 = sha256(canonicalJsonBytes(value.entries.map(
    // preserve the same actual-member tuple used by the transfer page
    (entry) => ({
      gapMemberSha256: entry.gapMemberSha256,
      payloadMemberSha256: entry.payloadIdentitySha256,
      stageReceiptMemberSha256: entry.stageReceiptMemberSha256,
      ...(Object.hasOwn(entry, "rainControlStateIdentitySha256")
        ? {
            rainControlStateMemberSha256: entry.rainControlStateIdentitySha256,
            rainControlStateStageReceiptMemberSha256:
              entry.rainControlStateStageReceiptMemberSha256,
          }
        : {}),
    }),
  )));
  // reject a checkpoint detached from its exact value-bearing members
  if (value.memberRootSha256 !== expectedMemberRootSha256) {
    throw new Error("adjustment revision gap checkpoint member root differs");
  }
  return Object.freeze(value);
}

// require one explicit canonical private archive root
function requireRevisionArchiveRoot(value) {
  // reject path coercion and empty configured roots
  if (typeof value !== "string" || value.length < 1) {
    throw new TypeError("adjustment revision archive root is invalid");
  }
  return resolve(value);
}

// require every fixed cold-reader directory without creating or repairing it
async function requireRevisionColdDirectories(root) {
  await requirePrivateDirectory(root);
  // validate only fixed literal children under the same canonical root
  for (const directory of [
    "revision-commit-receipts", "revision-frontier-successors",
    "revision-projections", "shadow-revision-capsules", "rain-control-states",
    "rain-control-state-stage-receipts",
  ]) {
    await requirePrivateDirectory(join(root, directory), root);
  }
}

// require every fixed successful-custody and retirement directory
async function requireRevisionCustodyDirectories(root) {
  await requireRevisionColdDirectories(root);
  // refuse missing or substituted auxiliary roots without reader-side repair
  for (const directory of [
    "revision-custody-acknowledgements", "revision-gaps",
    "revision-publish-receipts", "revision-spool-locks", "revision-stage-receipts",
    "shadow-revision-gaps", "shadow-revision-publish-receipts",
  ]) {
    await requirePrivateDirectory(join(root, directory), root);
  }
}

// read at most the bounded current and previous custody checkpoints
async function readRevisionCustodyAcknowledgements(root) {
  const directory = join(root, "revision-custody-acknowledgements");
  const names = (await readdir(directory)).sort();
  // reject hidden, temporary or caller-created state in the fixed checkpoint root
  if (names.length > 2 || names.some((name) =>
    name !== "current.json" && name !== "previous.json")) {
    throw new Error("adjustment revision custody state differs");
  }
  const currentBytes = await readOptionalPrivateFile(
    join(directory, "current.json"),
    ADJUSTMENT_REVISION_CUSTODY_ACK_MAXIMUM_BYTES,
  );
  const previousBytes = await readOptionalPrivateFile(
    join(directory, "previous.json"),
    ADJUSTMENT_REVISION_CUSTODY_ACK_MAXIMUM_BYTES,
  );
  // reject an orphan previous state instead of adopting it as authority
  if (currentBytes === null && previousBytes !== null) {
    throw new Error("adjustment revision custody state differs");
  }
  const current = currentBytes === null ? null :
    parseRevisionCustodyAcknowledgement(currentBytes);
  const previous = previousBytes === null ? null :
    parseRevisionCustodyAcknowledgement(previousBytes);
  // require the retained prior checkpoint to be the current direct predecessor
  if ((current === null && previous !== null) ||
    (current !== null &&
      ((previous === null) !== (current.previousAcknowledgementSha256 === null) ||
        (previous !== null && current.previousAcknowledgementSha256 !==
          previous.acknowledgementSha256)))) {
    throw new Error("adjustment revision custody predecessor differs");
  }
  return { current, currentBytes, previous };
}

// parse one canonical custody acknowledgement file
function parseRevisionCustodyAcknowledgement(bytes) {
  const parsed = JSON.parse(bytes.toString("utf8"));
  // select only one frozen custody contract by its literal version
  const value = parsed.contractVersion ===
      ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION
    ? validateAdjustmentRevisionColdCustodyAcknowledgementV2(parsed)
    : validateAdjustmentRevisionColdCustodyAcknowledgement(parsed);
  // reject alternate serialization of one valid checkpoint value
  if (!bytes.equals(canonicalJsonBytes(value))) {
    throw new Error("adjustment revision custody bytes differ");
  }
  return value;
}

// census only fixed successful-spool names before any retirement
async function requireSuccessfulRevisionSpoolCensus(root) {
  for (const directory of [
    "revision-commit-receipts", "revision-frontier-successors",
    "revision-projections", "revision-publish-receipts", "revision-stage-receipts",
    "shadow-revision-capsules", "shadow-revision-publish-receipts",
    "rain-control-states", "rain-control-state-stage-receipts",
  ]) {
    const names = await readdir(join(root, directory));
    // retain no more than two canonical content-addressed files per hot class
    const pattern = directory === "revision-commit-receipts"
      ? /^(?:sha256-[a-f0-9]{64}|batch-sha256-[a-f0-9]{64})\.json$/u
      : /^sha256-[a-f0-9]{64}\.json$/u;
    if (names.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
      names.some((name) => !pattern.test(name))) {
      throw new Error("adjustment revision custody spool census differs");
    }
  }
}

// require only fixed gap-transfer children created by the revision archive store
async function requireRevisionGapTransferDirectories(root) {
  await requirePrivateDirectory(root);
  // reject missing or substituted archive children without repairing the root
  for (const directory of [
    "revision-commit-receipts", "revision-gap-transfer", "revision-gaps",
    "revision-projections", "revision-spool-locks", "revision-stage-receipts",
    "shadow-revision-gaps", "shadow-revision-stages", "rain-control-states",
    "rain-control-state-stage-receipts", "rain-control-state-gaps",
  ]) {
    await requirePrivateDirectory(join(root, directory), root);
  }
}

// read the sole compact acknowledged gap frontier or return its fixed genesis
async function readRevisionGapFrontier(root) {
  const path = join(root, "revision-gap-transfer", "current.json");
  const bytes = await readOptionalPrivateFile(path, 4096);
  // represent the empty frontier without creating mutable state on reads
  if (bytes === null) {
    return Object.freeze({
      acknowledgedAt: null,
      acknowledgementSha256: null,
      contractVersion: ADJUSTMENT_REVISION_GAP_ACK_CONTRACT_VERSION,
      frontierSha256: revisionGapGenesisFrontier(),
      graphManifestSha256: null,
      memberRootSha256: null,
      pageCount: "0",
      pageSha256: null,
      predecessorFrontierSha256: null,
    });
  }
  const value = JSON.parse(bytes.toString("utf8"));
  const validated = validateAdjustmentRevisionGapPayloadAcknowledgement(value);
  // bind the fixed state to one canonical byte representation
  if (!bytes.equals(canonicalJsonBytes(validated))) {
    throw new Error("adjustment revision gap frontier differs");
  }
  return validated;
}

// collect at most two staged values that already have one permanent categorical gap
async function collectAdjustmentRevisionGapEntries(root) {
  const revisionGaps = await readStoredRevisionGaps(root);
  const shadowGaps = await readStoredShadowRevisionGaps(root);
  const admittedIdentities = new Set();
  const receiptNames = await readdir(join(root, "revision-commit-receipts"));
  // reject foreign receipt names before using them to exclude admitted stages
  if (receiptNames.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    receiptNames.some((name) =>
      !/^(?:sha256-[a-f0-9]{64}|batch-sha256-[a-f0-9]{64})\.json$/u.test(name))) {
    throw new Error("adjustment revision gap receipt census differs");
  }
  // retain the exact projection identities already admitted to the global chain
  for (const name of receiptNames.filter((entry) => !entry.startsWith("batch-"))) {
    const receipt = await readStoredRevisionReceipt(root, name.slice(7, -5));
    admittedIdentities.add(receipt.projectionIdentitySha256);
  }
  // exclude every body represented by one complete grouped commit file
  for (const group of await readStoredRevisionBatchCommitGroups(root)) {
    admittedIdentities.add(group.projectionIdentitySha256);
  }
  const entries = [];
  const stateGapNames = await readdir(join(root, "rain-control-state-gaps"));
  // scan only the two explicitly abandoned monthly-state payload slots
  if (stateGapNames.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    stateGapNames.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
    throw new Error("adjustment rain control state gap census differs");
  }
  for (const name of stateGapNames.sort()) {
    const stateSha256 = name.slice(7, -5);
    const stateBytes = await readRegularFile(join(root, "rain-control-states", name),
      ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES);
    const receiptBytes = await readRegularFile(join(root,
      "rain-control-state-stage-receipts", name), 4096);
    const stageReceipt = parseRainControlStateStageReceipt(receiptBytes, stateSha256);
    const gap = parseRainControlStateGap(await readRegularFile(join(root,
      "rain-control-state-gaps", name), 4096), {
      reason: "projection_stage_failed",
      stageReceiptSha256: stageReceipt.stageReceiptSha256,
      stateSha256,
    });
    const state = JSON.parse(stateBytes.toString("utf8"));
    // require canonical state bytes before carrying them as a cold gap member
    if (!stateBytes.equals(canonicalJsonBytes(state)) || sha256(stateBytes) !== stateSha256 ||
      state.contractVersion !== "rain-maintenance-control-state/v1") {
      throw new Error("adjustment rain control abandoned state differs");
    }
    entries.push(createAdjustmentRevisionGapEntry({
      gap,
      payloadBytes: stateBytes,
      payloadKind: state.contractVersion,
      slotIdentitySha256: stateSha256,
      stageReceipt,
      stageReceiptBytes: receiptBytes,
    }));
  }
  const projectionNames = await readdir(join(root, "revision-projections"));
  // scan only the reviewed two online canonical payload slots
  if (projectionNames.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    projectionNames.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
    throw new Error("adjustment revision gap projection census differs");
  }
  for (const name of projectionNames.sort()) {
    const identitySha256 = name.slice(7, -5);
    // exclude every receipt-bearing value from the unqualified stream
    if (admittedIdentities.has(identitySha256)) {
      continue;
    }
    const matches = revisionGaps.filter(
      // locate one permanent gap for this exact staged body
      (gap) => gap.input.projectionIdentitySha256 === identitySha256,
    );
    // leave an in-flight stage untouched until its one permanent gap exists
    if (matches.length === 0) {
      continue;
    }
    // refuse two permanent records claiming one staged body
    if (matches.length !== 1) {
      throw new Error("adjustment revision staged gap is ambiguous");
    }
    const payloadBytes = await readRegularFile(
      join(root, "revision-projections", name),
      ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES,
    );
    const stageBytes = await readRegularFile(
      join(root, "revision-stage-receipts", name),
      4096,
    );
    const stageReceipt = parseRevisionStageReceipt(
      stageBytes,
      identitySha256,
      matches[0].input.projectionKind,
      identitySha256,
    );
    const projection = JSON.parse(payloadBytes.toString("utf8"));
    // preserve the staged body contract without granting receipt semantics
    if (!["adjustment-revision-projection/v1", "adjustment-revision-batch-projection/v2",
      "adjustment-rain-gate-feature-projection/v2",
      "adjustment-rain-gate-control-projection/v3",
      "adjustment-rain-fixed-gauge-target-projection/v1"]
      .includes(projection.contractVersion)) {
      throw new Error("adjustment revision staged gap projection differs");
    }
    const rainControl = projection.contractVersion ===
      "adjustment-rain-gate-control-projection/v3"
      ? await readStoredRainControlStateAuxiliary(root, {
          bytesBase64: payloadBytes.toString("base64"),
          identitySha256,
          kind: projection.contractVersion,
        })
      : null;
    entries.push(createAdjustmentRevisionGapEntry({
      gap: matches[0],
      payloadBytes,
      payloadKind: projection.contractVersion,
      ...(rainControl === null ? {} : rainControl),
      slotIdentitySha256: identitySha256,
      stageReceipt,
      stageReceiptBytes: stageBytes,
    }));
  }
  const shadowNames = await readdir(join(root, "shadow-revision-stages"));
  // scan only the remaining online shadow-stage payload slots
  if (shadowNames.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    shadowNames.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
    throw new Error("adjustment shadow staged gap census differs");
  }
  for (const name of shadowNames.sort()) {
    const stageBytes = await readRegularFile(
      join(root, "shadow-revision-stages", name),
      2 * 1024 * 1024,
    );
    const stage = parseShadowRevisionStage(stageBytes);
    const matches = shadowGaps.filter(
      // bind the frozen gap shape to the stage's scheduled registration identity
      (gap) => gap.input.dueKey === stage.input.metadata.dueKey &&
        gap.input.registrationSha256 === stage.input.metadata.registrationSha256,
    );
    // leave an in-flight stage untouched until its one permanent gap exists
    if (matches.length === 0) {
      continue;
    }
    // require one addressed stage and one permanent classification
    if (matches.length !== 1 ||
      name !== `sha256-${stage.input.metadata.predictionSha256}.json`) {
      throw new Error("adjustment shadow staged gap is ambiguous");
    }
    entries.push(createAdjustmentRevisionGapEntry({
      gap: matches[0],
      payloadBytes: stageBytes,
      payloadKind: stage.contractVersion,
      slotIdentitySha256: stage.input.metadata.predictionSha256,
      stageReceipt: stage.stageReceipt,
      stageReceiptBytes: canonicalJsonBytes(stage.stageReceipt),
    }));
  }
  // preserve stable gap-member order without assigning any server ordinal
  return entries.sort((left, right) =>
    left.gap.gapSha256.localeCompare(right.gap.gapSha256, "en"));
}

// read every bounded canonical projection gap once for staged association
async function readStoredRevisionGaps(root) {
  const names = await readdir(join(root, "revision-gaps"));
  // enforce the same fixed metadata population admitted by the writer
  if (names.length > 4096 ||
    names.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
    throw new Error("adjustment revision gap census differs");
  }
  const values = [];
  // validate each permanent gap before using its body identity
  for (const name of names) {
    const bytes = await readRegularFile(join(root, "revision-gaps", name), 4096);
    const value = parseRevisionGap(bytes);
    // bind the value-free gap to its content-addressed filename
    if (name !== `sha256-${sha256(canonicalJsonBytes({
      logicalKeySha256: value.input.logicalKeySha256,
      projectionIdentitySha256: value.input.projectionIdentitySha256,
    }))}.json`) {
      throw new Error("adjustment revision gap address differs");
    }
    values.push(value);
  }
  return values;
}

// read every bounded shadow gap once for staged association
async function readStoredShadowRevisionGaps(root) {
  const names = await readdir(join(root, "shadow-revision-gaps"));
  // enforce the same fixed metadata population admitted by the writer
  if (names.length > 4096 ||
    names.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
    throw new Error("adjustment shadow gap census differs");
  }
  const values = [];
  // validate each permanent shadow gap before using its schedule identity
  for (const name of names) {
    const bytes = await readRegularFile(join(root, "shadow-revision-gaps", name), 4096);
    const value = parseShadowRevisionGap(bytes);
    const identitySha256 = sha256(canonicalJsonBytes({
      dueKey: value.input.dueKey,
      registrationSha256: value.input.registrationSha256,
    }));
    // bind the scheduled gap to its content-addressed filename
    if (name !== `sha256-${identitySha256}.json`) {
      throw new Error("adjustment shadow gap address differs");
    }
    values.push(value);
  }
  return values;
}

// build one portable gap entry from exact retained file bytes
function createAdjustmentRevisionGapEntry(input) {
  return {
    gap: input.gap,
    gapMemberSha256: sha256(canonicalJsonBytes(input.gap)),
    payload: {
      bytesBase64: input.payloadBytes.toString("base64"),
      identitySha256: sha256(input.payloadBytes),
      kind: input.payloadKind,
    },
    ...(input.rainControlState === undefined
      ? {}
      : {
          rainControlState: input.rainControlState,
          rainControlStateStageReceipt: input.rainControlStateStageReceipt,
          rainControlStateStageReceiptMemberSha256:
            sha256(canonicalJsonBytes(input.rainControlStateStageReceipt)),
        }),
    slotIdentitySha256: input.slotIdentitySha256,
    stageReceipt: input.stageReceipt,
    stageReceiptMemberSha256: sha256(input.stageReceiptBytes),
  };
}

// validate one canonical or shadow gap entry without upgrading it to a receipt
function validateAdjustmentRevisionGapEntry(entry) {
  const rainControl = entry?.payload?.kind ===
    "adjustment-rain-gate-control-projection/v3";
  requireExactKeys(entry, [
    "gap", "gapMemberSha256", "payload", "slotIdentitySha256", "stageReceipt",
    "stageReceiptMemberSha256", ...(rainControl ? ["rainControlState",
      "rainControlStateStageReceipt", "rainControlStateStageReceiptMemberSha256"] : []),
  ], "adjustment revision gap entry");
  requireSha256(entry.gapMemberSha256, "adjustment revision gap member");
  requireSha256(entry.slotIdentitySha256, "adjustment revision gap slot");
  requireSha256(entry.stageReceiptMemberSha256,
    "adjustment revision gap stage receipt member");
  // validate the optional monthly-state member address before parsing its bytes
  if (rainControl) {
    requireSha256(entry.rainControlStateStageReceiptMemberSha256,
      "adjustment revision gap rain control receipt member");
  }
  requireExactKeys(entry.payload, ["bytesBase64", "identitySha256", "kind"],
    "adjustment revision gap payload");
  requireSha256(entry.payload.identitySha256, "adjustment revision gap payload identity");
  const payloadBytes = decodeCanonicalBase64(
    entry.payload.bytesBase64,
    "adjustment revision gap payload",
  );
  // select one closed producer grammar without assigning receipt semantics
  if (["adjustment-revision-projection/v1", "adjustment-revision-batch-projection/v2",
      "adjustment-rain-gate-feature-projection/v2",
      "adjustment-rain-gate-control-projection/v3",
      "adjustment-rain-fixed-gauge-target-projection/v1",
      "adjustment-temperature-native-source-projection/v2"]
    .includes(entry.payload.kind)) {
    const gap = parseRevisionGap(canonicalJsonBytes(entry.gap));
    parseRevisionStageReceipt(
      canonicalJsonBytes(entry.stageReceipt),
      entry.payload.identitySha256,
      gap.input.projectionKind,
      entry.payload.identitySha256,
    );
    // bind the durable gap, slot and stage to one canonical body hash
    if (gap.input.projectionIdentitySha256 !== entry.payload.identitySha256 ||
      gap.input.projectionSha256 !== entry.payload.identitySha256 ||
      entry.slotIdentitySha256 !== entry.payload.identitySha256) {
      throw new Error("adjustment revision gap projection differs");
    }
    // require the complete archived public state before admitting a control gap
    if (rainControl) {
      validateRevisionRainControlColdAuxiliary(entry);
    }
  } else if (entry.payload.kind === "rain-maintenance-control-state/v1") {
    const gap = parseRainControlStateGap(canonicalJsonBytes(entry.gap), {
      reason: "projection_stage_failed",
      stageReceiptSha256: entry.stageReceipt.stageReceiptSha256,
      stateSha256: entry.slotIdentitySha256,
    });
    const state = JSON.parse(payloadBytes.toString("utf8"));
    parseRainControlStateStageReceipt(canonicalJsonBytes(entry.stageReceipt),
      entry.slotIdentitySha256);
    // bind the permanent gap, actual state bytes and first receipt without an ordinal
    if (!payloadBytes.equals(canonicalJsonBytes(state)) ||
      state.contractVersion !== entry.payload.kind ||
      entry.payload.identitySha256 !== entry.slotIdentitySha256 ||
      gap.stateSha256 !== entry.slotIdentitySha256) {
      throw new Error("adjustment rain control state gap differs");
    }
  } else if (["adjustment-shadow-revision-stage/v1",
    "adjustment-shadow-revision-stage/v2"].includes(entry.payload.kind)) {
    const gap = parseShadowRevisionGap(canonicalJsonBytes(entry.gap));
    const stage = parseShadowRevisionStage(payloadBytes);
    // bind the durable gap and slot to the complete shadow stage bytes
    if (stage.contractVersion !== entry.payload.kind ||
      canonicalJson(stage.stageReceipt) !== canonicalJson(entry.stageReceipt) ||
      gap.input.dueKey !== stage.input.metadata.dueKey ||
      gap.input.registrationSha256 !== stage.input.metadata.registrationSha256 ||
      entry.slotIdentitySha256 !== stage.input.metadata.predictionSha256) {
      throw new Error("adjustment shadow gap stage differs");
    }
  } else {
    throw new TypeError("adjustment revision gap payload kind is invalid");
  }
  // bind every graph-member hash to the exact transported bytes
  if (entry.payload.identitySha256 !== sha256(payloadBytes) ||
    entry.gapMemberSha256 !== sha256(canonicalJsonBytes(entry.gap)) ||
    entry.stageReceiptMemberSha256 !== sha256(canonicalJsonBytes(entry.stageReceipt)) ||
    (rainControl && entry.rainControlStateStageReceiptMemberSha256 !==
      sha256(canonicalJsonBytes(entry.rainControlStateStageReceipt)))) {
    throw new Error("adjustment revision gap member identity differs");
  }
  return entry;
}

// build one page identity over its exact unqualified member set
function buildAdjustmentRevisionGapPayloadPage(input) {
  const entries = input.entries.map(
    // normalize only validated closed entries
    (entry) => validateAdjustmentRevisionGapEntry(entry),
  );
  const unsigned = {
    contractVersion: ADJUSTMENT_REVISION_GAP_PAGE_CONTRACT_VERSION,
    entries,
    idle: entries.length === 0,
    memberRootSha256: revisionGapMemberRoot(entries),
    predecessorFrontierSha256: input.predecessorFrontierSha256,
    startSha256: input.startSha256,
  };
  return { ...unsigned, pageSha256: sha256(canonicalJsonBytes(unsigned)) };
}

// hash exact cold-member bytes instead of caller labels or file paths
function revisionGapMemberRoot(entries) {
  return sha256(canonicalJsonBytes(entries.map(
    // retain every exact graph member hash in stable page order
    (entry) => ({
      gapMemberSha256: entry.gapMemberSha256,
      payloadMemberSha256: entry.payload.identitySha256,
      stageReceiptMemberSha256: entry.stageReceiptMemberSha256,
      ...(Object.hasOwn(entry, "rainControlState")
        ? {
            rainControlStateMemberSha256: entry.rainControlState.identitySha256,
            rainControlStateStageReceiptMemberSha256:
              entry.rainControlStateStageReceiptMemberSha256,
          }
        : {}),
    }),
  )));
}

// unlink only staged values already covered by the acknowledged exact cold graph
async function retireAcknowledgedRevisionGapSlots(root, page) {
  validateAdjustmentRevisionGapPayloadPage(page);
  // remove only files whose bytes still match the frozen page members
  for (const entry of page.entries) {
    // select the only two reviewed online slot layouts
    if (["adjustment-revision-projection/v1", "adjustment-revision-batch-projection/v2",
      "adjustment-rain-gate-feature-projection/v2",
      "adjustment-rain-gate-control-projection/v3",
      "adjustment-rain-fixed-gauge-target-projection/v1",
      "adjustment-temperature-native-source-projection/v2"]
      .includes(entry.payload.kind)) {
      const payloadPath = join(root, "revision-projections",
        `sha256-${entry.slotIdentitySha256}.json`);
      const stagePath = join(root, "revision-stage-receipts",
        `sha256-${entry.slotIdentitySha256}.json`);
      await verifyOptionalRetirementFile(payloadPath, entry.payload.identitySha256, ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES);
      await verifyOptionalRetirementFile(stagePath, entry.stageReceiptMemberSha256, 4096);
      await unlink(payloadPath).catch(ignoreAbsentRetirementFile);
      await unlink(stagePath).catch(ignoreAbsentRetirementFile);
      await fsyncDirectory(dirname(payloadPath));
      await fsyncDirectory(dirname(stagePath));
      // retire both state auxiliaries only with their exact cold graph members
      if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
        const statePath = join(root, "rain-control-states",
          `sha256-${entry.rainControlState.stateSha256}.json`);
        const stateReceiptPath = join(root, "rain-control-state-stage-receipts",
          `sha256-${entry.rainControlState.stateSha256}.json`);
        await verifyOptionalRetirementFile(statePath,
          entry.rainControlState.identitySha256,
          ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES);
        await verifyOptionalRetirementFile(stateReceiptPath,
          entry.rainControlStateStageReceiptMemberSha256, 4096);
        await unlink(statePath).catch(ignoreAbsentRetirementFile);
        await unlink(stateReceiptPath).catch(ignoreAbsentRetirementFile);
        await fsyncDirectory(dirname(statePath));
        await fsyncDirectory(dirname(stateReceiptPath));
      }
    } else if (entry.payload.kind === "rain-maintenance-control-state/v1") {
      const statePath = join(root, "rain-control-states",
        `sha256-${entry.slotIdentitySha256}.json`);
      const receiptPath = join(root, "rain-control-state-stage-receipts",
        `sha256-${entry.slotIdentitySha256}.json`);
      const gapPath = join(root, "rain-control-state-gaps",
        `sha256-${entry.slotIdentitySha256}.json`);
      await verifyOptionalRetirementFile(statePath, entry.payload.identitySha256,
        ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES);
      await verifyOptionalRetirementFile(receiptPath,
        entry.stageReceiptMemberSha256, 4096);
      await verifyOptionalRetirementFile(gapPath, entry.gapMemberSha256, 4096);
      await unlink(statePath).catch(ignoreAbsentRetirementFile);
      await unlink(receiptPath).catch(ignoreAbsentRetirementFile);
      await unlink(gapPath).catch(ignoreAbsentRetirementFile);
      await fsyncDirectory(dirname(statePath));
      await fsyncDirectory(dirname(receiptPath));
      await fsyncDirectory(dirname(gapPath));
    } else {
      const stagePath = join(root, "shadow-revision-stages",
        `sha256-${entry.slotIdentitySha256}.json`);
      await verifyOptionalRetirementFile(stagePath, entry.payload.identitySha256,
        2 * 1024 * 1024);
      await unlink(stagePath).catch(ignoreAbsentRetirementFile);
      await fsyncDirectory(dirname(stagePath));
    }
  }
}

// retire only files enumerated by one durable custody acknowledgement
async function retireAdjustmentRevisionCustodyEntries(root, entriesValue) {
  const entries = entriesValue.map(
    // revalidate durable checkpoint values before deriving any path
    (entry) => validateAdjustmentRevisionCustodyRetirementEntry(entry),
  );
  const files = entries.map((entry) => adjustmentRevisionCustodyRetirementFile(root, entry));
  // verify the complete retirement set before deleting its first file
  for (const file of files) {
    await verifyOptionalCustodyRetirementFile(
      file.path,
      file.entry.fileSha256,
      file.maximumBytes,
    );
  }
  const directories = new Set();
  // unlink only the exact already-verified redundant representations
  for (const file of files) {
    await unlink(file.path).catch(ignoreAbsentRetirementFile);
    directories.add(dirname(file.path));
  }
  // make every exact unlink durable before acknowledging completion
  for (const directory of directories) {
    await fsyncDirectory(directory);
  }
}

// map one closed retirement kind to its sole fixed hot path
function adjustmentRevisionCustodyRetirementFile(root, entry) {
  const layouts = {
    rain_control_state: ["rain-control-states", ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
      "sha256-"],
    rain_control_state_stage_receipt: ["rain-control-state-stage-receipts", 4096,
      "sha256-"],
    revision_commit_group: ["revision-commit-receipts", 256 * 1024, "batch-sha256-"],
    revision_commit_receipt: ["revision-commit-receipts", 4096, "sha256-"],
    revision_frontier_successor: ["revision-frontier-successors", 4096, "sha256-"],
    revision_projection: ["revision-projections", ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES, "sha256-"],
    revision_publish_receipt: ["revision-publish-receipts", 32 * 1024, "sha256-"],
    revision_stage_receipt: ["revision-stage-receipts", 4096, "sha256-"],
    revision_terminal_gap: ["revision-gaps", 4096, "sha256-"],
    shadow_revision_capsule: ["shadow-revision-capsules", 2 * 1024 * 1024, "sha256-"],
    shadow_revision_publish_receipt: ["shadow-revision-publish-receipts", 4096, "sha256-"],
    shadow_revision_terminal_gap: ["shadow-revision-gaps", 4096, "sha256-"],
  };
  const layout = layouts[entry.kind];
  // refuse any kind that escaped the closed acknowledgement validator
  if (layout === undefined) {
    throw new TypeError("adjustment revision custody retirement layout is invalid");
  }
  return {
    entry,
    maximumBytes: layout[1],
    path: join(root, layout[0], `${layout[2]}${entry.identitySha256}.json`),
  };
}

// permit only an exact retained file or one crash-resumed absence
async function verifyOptionalCustodyRetirementFile(path, expectedSha256, maximumBytes) {
  const bytes = await readOptionalPrivateFile(path, maximumBytes);
  // reject substitution before any other page member is removed
  if (bytes !== null && sha256(bytes) !== expectedSha256) {
    throw new Error("adjustment revision custody retirement file differs");
  }
}

// require one retained retirement file to match or one crash-resumed absence
async function verifyOptionalRetirementFile(path, expectedSha256, maximumBytes) {
  const bytes = await readOptionalPrivateFile(path, maximumBytes);
  // reject substitution while permitting a post-frontier crash retry
  if (bytes !== null && sha256(bytes) !== expectedSha256) {
    throw new Error("adjustment revision gap retirement file differs");
  }
}

// ignore only a file already removed after the durable acknowledgement
function ignoreAbsentRetirementFile(error) {
  if (error?.code !== "ENOENT") {
    throw error;
  }
}

// return the fixed non-ordinal gap frontier genesis identity
function revisionGapGenesisFrontier() {
  return sha256(Buffer.from("adjustment-revision-gap-frontier/v1\n0\n"));
}

// read one canonical receipt directly by its claimed frontier
async function readStoredRevisionReceipt(root, frontierSha256) {
  requireSha256(frontierSha256, "adjustment revision stored frontier");
  const bytes = await readRegularFile(join(root, "revision-commit-receipts",
    `sha256-${frontierSha256}.json`), 4096);
  const receipt = JSON.parse(bytes.toString("utf8"));
  validateRevisionCommitReceiptIdentity(receipt);
  // bind the address and one canonical byte representation
  if (receipt.frontierSha256 !== frontierSha256 ||
    !bytes.equals(canonicalJsonBytes(receipt))) {
    throw new Error("adjustment revision stored receipt differs");
  }
  return receipt;
}

// read one single or grouped direct successor without splitting a shared body
async function readStoredRevisionColdGroup(root, predecessorFrontierSha256) {
  requireSha256(predecessorFrontierSha256,
    "adjustment revision stored predecessor frontier");
  const individualPath = join(root, "revision-frontier-successors",
    `sha256-${predecessorFrontierSha256}.json`);
  const individual = await readOptionalPrivateFile(individualPath, 4096);
  // preserve the exact legacy single-receipt representation
  if (individual !== null) {
    const successor = await readStoredRevisionSuccessor(root, predecessorFrontierSha256);
    const receipt = await readStoredRevisionReceipt(root, successor.frontierSha256);
    return Object.freeze({ batch: false, receipts: [receipt], successors: [successor] });
  }
  const groups = await readStoredRevisionBatchCommitGroups(root);
  const matches = groups.filter(
    // select only a group whose first receipt directly follows the cursor
    (group) => group.receipts[0].predecessorFrontierSha256 === predecessorFrontierSha256,
  );
  if (matches.length !== 1) {
    throw new Error("adjustment revision cold successor group differs");
  }
  return Object.freeze({
    batch: true,
    receipts: matches[0].receipts,
    successors: matches[0].successors,
  });
}

// read at most two canonical grouped receipt files from the hot payload spool
async function readStoredRevisionBatchCommitGroups(root) {
  const names = await readdir(join(root, "revision-commit-receipts"));
  // permit only legacy single files or the additive grouped body address
  if (names.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    names.some((name) =>
      !/^(?:sha256-[a-f0-9]{64}|batch-sha256-[a-f0-9]{64})\.json$/u.test(name))) {
    throw new Error("adjustment revision receipt census differs");
  }
  const groups = [];
  // parse only the grouped members while leaving v1 files untouched
  for (const name of names.filter((entry) => entry.startsWith("batch-"))) {
    const bytes = await readRegularFile(
      join(root, "revision-commit-receipts", name),
      256 * 1024,
    );
    const group = validateRevisionBatchCommitGroup(JSON.parse(bytes.toString("utf8")));
    if (!bytes.equals(canonicalJsonBytes(group)) ||
      name !== `batch-sha256-${group.projectionIdentitySha256}.json`) {
      throw new Error("adjustment revision batch commit group bytes differ");
    }
    groups.push(group);
  }
  return groups;
}

// read one unique direct successor for a verified cold frontier
async function readStoredRevisionSuccessor(root, predecessorFrontierSha256) {
  requireSha256(predecessorFrontierSha256,
    "adjustment revision stored predecessor frontier");
  const bytes = await readRegularFile(join(root, "revision-frontier-successors",
    `sha256-${predecessorFrontierSha256}.json`), 4096);
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "archiveCommitOrdinal", "contractVersion", "frontierSha256",
    "predecessorFrontierSha256", "receiptSha256",
  ], "adjustment revision successor");
  requireUint64Text(value.archiveCommitOrdinal, "adjustment revision successor ordinal");
  for (const field of ["frontierSha256", "predecessorFrontierSha256", "receiptSha256"]) {
    requireSha256(value[field], `adjustment revision successor ${field}`);
  }
  // bind the successor to its address and canonical immutable bytes
  if (value.contractVersion !== "adjustment-revision-successor/v1" ||
    value.predecessorFrontierSha256 !== predecessorFrontierSha256 ||
    !bytes.equals(canonicalJsonBytes(value))) {
    throw new Error("adjustment revision successor differs");
  }
  return value;
}

// read one exact canonical or shadow value payload for its database receipt
async function readStoredRevisionPayload(root, receipt) {
  let bytes;
  let kind;
  // select the distinct shadow capsule grammar without relabelling its hashes
  if (receipt.projectionKind === "shadow_prediction") {
    bytes = await readRegularFile(join(root, "shadow-revision-capsules",
      `sha256-${receipt.receiptSha256}.json`), 2 * 1024 * 1024);
    const capsule = parseShadowRevisionCapsule(bytes);
    // require the archived capsule to carry this exact server receipt
    if (canonicalJson(capsule.revisionReceipt) !== canonicalJson(receipt)) {
      throw new Error("adjustment shadow revision capsule receipt differs");
    }
    kind = capsule.contractVersion;
  } else {
    bytes = await readRegularFile(join(root, "revision-projections",
      `sha256-${receipt.projectionIdentitySha256}.json`), ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES);
    // bind exact canonical projection bytes to both receipt body hashes
    if (sha256(bytes) !== receipt.projectionIdentitySha256 ||
      receipt.projectionSha256 !== receipt.projectionIdentitySha256) {
      throw new Error("adjustment revision projection bytes differ");
    }
    const document = JSON.parse(bytes.toString("utf8"));
    // preserve the exact staged projection contract as the cold member kind
    if (!["adjustment-revision-projection/v1", "adjustment-revision-batch-projection/v2",
      "adjustment-rain-gate-feature-projection/v2",
      "adjustment-rain-gate-control-projection/v3",
      "adjustment-rain-fixed-gauge-target-projection/v1",
      "adjustment-temperature-native-source-projection/v2"]
      .includes(document.contractVersion)) {
      throw new Error("adjustment revision projection contract differs");
    }
    kind = document.contractVersion;
  }
  return Object.freeze({
    bytesBase64: bytes.toString("base64"),
    identitySha256: sha256(bytes),
    kind,
  });
}

// read exact first-stage, publication and successor metadata made redundant by cold graph
async function readStoredRevisionAuxiliary(root, receipt, payload, successor) {
  let stageReceipt;
  let publication;
  // recover shadow metadata from its complete retained capsule
  if (receipt.projectionKind === "shadow_prediction") {
    const capsule = parseShadowRevisionCapsule(Buffer.from(payload.bytesBase64, "base64"));
    stageReceipt = capsule.stageReceipt;
    const publishBytes = await readOptionalPrivateFile(
      join(root, "shadow-revision-publish-receipts",
        `sha256-${capsule.metadata.predictionSha256}.json`),
      4096,
    );
    // preserve a genuine publish receipt or one exact permanent terminal gap
    if (publishBytes !== null) {
      publication = {
        disposition: "published",
        value: parseShadowRevisionPublishReceipt(
          publishBytes,
          capsule.metadata.predictionSha256,
          capsule.comparatorSha256 ?? null,
        ),
      };
    } else {
      const gaps = (await readStoredShadowRevisionGaps(root)).filter(
        // match the scheduled source/body capture without caller labels
        (gap) => gap.input.dueKey === capsule.metadata.dueKey &&
          gap.input.registrationSha256 === capsule.metadata.registrationSha256 &&
          gap.input.reason === "archive_publish_failed",
      );
      // require one durable permanent gap before advancing the ordinal chain
      if (gaps.length !== 1) {
        throw new Error("adjustment shadow revision terminal publication differs");
      }
      publication = { disposition: "committed_unpublished_gap", value: gaps[0] };
    }
  } else {
    stageReceipt = parseRevisionStageReceipt(
      await readRegularFile(join(root, "revision-stage-receipts",
        `sha256-${receipt.projectionIdentitySha256}.json`), 4096),
      receipt.projectionIdentitySha256,
      receipt.projectionKind,
      receipt.projectionSha256,
    );
    const publishBytes = await readOptionalPrivateFile(
      join(root, "revision-publish-receipts",
        `sha256-${receipt.projectionIdentitySha256}.json`),
      4096,
    );
    // preserve a genuine publish receipt or one exact permanent terminal gap
    if (publishBytes !== null) {
      publication = {
        disposition: "published",
        value: parseRevisionPublishReceipt(publishBytes, receipt),
      };
    } else {
      const gaps = (await readStoredRevisionGaps(root)).filter(
        // bind the terminal gap to this exact receipt-bearing body
        (gap) => gap.input.projectionIdentitySha256 ===
          receipt.projectionIdentitySha256 &&
          gap.input.projectionKind === receipt.projectionKind &&
          gap.input.reason === "archive_publish_failed",
      );
      // require one durable permanent gap before advancing the ordinal chain
      if (gaps.length !== 1) {
        throw new Error("adjustment revision terminal publication differs");
      }
      publication = { disposition: "committed_unpublished_gap", value: gaps[0] };
    }
  }
  return { publication, stageReceipt, successor };
}

// read one exact monthly rain state and its first durable availability receipt
async function readStoredRainControlStateAuxiliary(root, payload) {
  const projectionBytes = Buffer.from(payload.bytesBase64, "base64");
  const projection = JSON.parse(projectionBytes.toString("utf8"));
  // select only the additive control projection that names this auxiliary
  if (projection.contractVersion !== "adjustment-rain-gate-control-projection/v3") {
    throw new TypeError("adjustment rain control cold projection is invalid");
  }
  requireSha256(projection.stateSha256, "adjustment rain control cold state identity");
  requireSha256(projection.stateStageReceiptSha256,
    "adjustment rain control cold stage identity");
  const stateBytes = await readRegularFile(join(root, "rain-control-states",
    `sha256-${projection.stateSha256}.json`), ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES);
  const state = JSON.parse(stateBytes.toString("utf8"));
  const stageReceipt = parseRainControlStateStageReceipt(
    await readRegularFile(join(root, "rain-control-state-stage-receipts",
      `sha256-${projection.stateSha256}.json`), 4096),
    projection.stateSha256,
  );
  // preserve exact canonical public state bytes and every embedded crossbinding
  if (!stateBytes.equals(canonicalJsonBytes(state)) ||
    state.contractVersion !== "rain-maintenance-control-state/v1" ||
    sha256(stateBytes) !== projection.stateSha256 ||
    state.ordinalArtifactSha256 !== projection.ordinalArtifactSha256 ||
    stageReceipt.stageReceiptSha256 !== projection.stateStageReceiptSha256) {
    throw new Error("adjustment rain control cold state differs");
  }
  return {
    rainControlState: Object.freeze({
      bytesBase64: stateBytes.toString("base64"),
      identitySha256: sha256(stateBytes),
      kind: state.contractVersion,
      stateSha256: projection.stateSha256,
    }),
    rainControlStateStageReceipt: stageReceipt,
  };
}

// read one grouped body's shared stage and all-receipt publication proof
async function readStoredRevisionBatchAuxiliary(root, receipts, payload) {
  const first = receipts[0];
  const projection = JSON.parse(Buffer.from(payload.bytesBase64, "base64").toString("utf8"));
  if (!["adjustment-revision-batch-projection/v2",
    "adjustment-rain-fixed-gauge-target-projection/v1"]
    .includes(projection.contractVersion) ||
    projection.rows?.length !== receipts.length) {
    throw new Error("adjustment revision batch cold payload differs");
  }
  const stageReceipt = parseRevisionStageReceipt(
    await readRegularFile(join(root, "revision-stage-receipts",
      `sha256-${first.projectionIdentitySha256}.json`), 4096),
    first.projectionIdentitySha256,
    first.projectionKind,
    first.projectionSha256,
  );
  const publishBytes = await readOptionalPrivateFile(
    join(root, "revision-publish-receipts",
      `sha256-${first.projectionIdentitySha256}.json`),
    32 * 1024,
  );
  let publication;
  // preserve one complete grouped publication or its permanent terminal gap
  if (publishBytes !== null) {
    publication = {
      disposition: "published",
      value: parseRevisionBatchPublishReceipt(publishBytes, receipts),
    };
  } else {
    const gaps = (await readStoredRevisionGaps(root)).filter(
      // bind the terminal gap to this exact grouped body
      (gap) => gap.input.projectionIdentitySha256 === first.projectionIdentitySha256 &&
        gap.input.projectionKind === first.projectionKind &&
        gap.input.reason === "archive_publish_failed",
    );
    if (gaps.length !== 1) {
      throw new Error("adjustment revision batch terminal publication differs");
    }
    publication = { disposition: "committed_unpublished_gap", value: gaps[0] };
  }
  return { publication, stageReceipt };
}

// build one complete immutable page around its next verified cursor
function buildAdjustmentRevisionColdPage(input) {
  const unsigned = adjustmentRevisionColdPageUnsigned({
    afterArchiveCommitOrdinal: input.afterArchiveCommitOrdinal,
    afterFrontierSha256: input.afterFrontierSha256,
    contractVersion: ADJUSTMENT_REVISION_COLD_PAGE_CONTRACT_VERSION,
    entries: input.entries,
    eof: input.nextArchiveCommitOrdinal === input.watermarkArchiveCommitOrdinal &&
      input.nextFrontierSha256 === input.watermarkFrontierSha256,
    nextArchiveCommitOrdinal: input.nextArchiveCommitOrdinal,
    nextFrontierSha256: input.nextFrontierSha256,
    previousPageSha256: input.previousPageSha256,
    startSha256: input.startSha256,
    watermarkArchiveCommitOrdinal: input.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: input.watermarkFrontierSha256,
  });
  return {
    ...unsigned,
    pageSha256: sha256(canonicalJsonBytes(unsigned)),
  };
}

// project only the fields covered by one page identity
function adjustmentRevisionColdPageUnsigned(value) {
  return {
    afterArchiveCommitOrdinal: value.afterArchiveCommitOrdinal,
    afterFrontierSha256: value.afterFrontierSha256,
    contractVersion: value.contractVersion,
    entries: value.entries,
    eof: value.eof,
    nextArchiveCommitOrdinal: value.nextArchiveCommitOrdinal,
    nextFrontierSha256: value.nextFrontierSha256,
    previousPageSha256: value.previousPageSha256,
    startSha256: value.startSha256,
    watermarkArchiveCommitOrdinal: value.watermarkArchiveCommitOrdinal,
    watermarkFrontierSha256: value.watermarkFrontierSha256,
  };
}

// verify one page payload against its exact server receipt grammar
function validateRevisionColdPayload(value, receipt) {
  requireExactKeys(value, ["bytesBase64", "identitySha256", "kind"],
    "adjustment revision cold payload");
  requireSha256(value.identitySha256, "adjustment revision cold payload identity");
  const bytes = decodeCanonicalBase64(value.bytesBase64,
    "adjustment revision cold payload");
  // preserve the distinct shadow capsule without imposing canonical body equality
  if (receipt.projectionKind === "shadow_prediction") {
    const capsule = parseShadowRevisionCapsule(bytes);
    if (value.kind !== capsule.contractVersion ||
      canonicalJson(capsule.revisionReceipt) !== canonicalJson(receipt)) {
      throw new Error("adjustment shadow revision cold payload differs");
    }
  } else {
    const document = JSON.parse(bytes.toString("utf8"));
    // bind v1 or additive grouped-v2 kind to the exact staged body
    if (!["adjustment-revision-projection/v1", "adjustment-revision-batch-projection/v2",
      "adjustment-rain-gate-feature-projection/v2",
      "adjustment-rain-gate-control-projection/v3",
      "adjustment-rain-fixed-gauge-target-projection/v1",
      "adjustment-temperature-native-source-projection/v2"]
      .includes(document.contractVersion) || value.kind !== document.contractVersion ||
      receipt.projectionIdentitySha256 !== value.identitySha256 ||
      receipt.projectionSha256 !== value.identitySha256) {
      throw new Error("adjustment revision cold payload differs");
    }
  }
  // bind the portable member identity to exact decoded bytes in both grammars
  if (value.identitySha256 !== sha256(bytes)) {
    throw new Error("adjustment revision cold payload identity differs");
  }
  return value;
}

// normalize one legacy single or additive grouped cold-page entry
function revisionColdEntryParts(entry) {
  const batch = Object.hasOwn(entry, "receipts");
  if (batch) {
    requireExactKeys(entry, [
      "payload", "publication", "receipts", "stageReceipt", "successors",
    ], "adjustment revision cold batch entry");
    if (!Array.isArray(entry.receipts) || !Array.isArray(entry.successors) ||
      entry.receipts.length < 1 ||
      entry.receipts.length > ADJUSTMENT_REVISION_BATCH_MAXIMUM_RECEIPTS ||
      entry.successors.length !== entry.receipts.length) {
      throw new TypeError("adjustment revision cold batch entry is invalid");
    }
    return { batch: true, receipts: entry.receipts, successors: entry.successors };
  }
  const rainControl = entry?.payload?.kind ===
    "adjustment-rain-gate-control-projection/v3";
  requireExactKeys(entry, [
    "payload", "publication",
    ...(rainControl ? ["rainControlState", "rainControlStateStageReceipt"] : []),
    "receipt", "stageReceipt", "successor",
  ], "adjustment revision cold page entry");
  return { batch: false, receipts: [entry.receipt], successors: [entry.successor] };
}

// verify one page entry's exact durable stage, publication and successor metadata
function validateRevisionColdAuxiliary(entry) {
  requireExactKeys(entry.successor, [
    "archiveCommitOrdinal", "contractVersion", "frontierSha256",
    "predecessorFrontierSha256", "receiptSha256",
  ], "adjustment revision cold successor");
  // bind the direct successor index to the included authoritative receipt
  if (entry.successor.contractVersion !== "adjustment-revision-successor/v1" ||
    entry.successor.archiveCommitOrdinal !== entry.receipt.archiveCommitOrdinal ||
    entry.successor.frontierSha256 !== entry.receipt.frontierSha256 ||
    entry.successor.predecessorFrontierSha256 !==
      entry.receipt.predecessorFrontierSha256 ||
    entry.successor.receiptSha256 !== entry.receipt.receiptSha256) {
    throw new Error("adjustment revision cold successor differs");
  }
  requireExactKeys(entry.publication, ["disposition", "value"],
    "adjustment revision cold publication");
  // select the distinct stage and publication grammars by producer class
  if (entry.receipt.projectionKind === "shadow_prediction") {
    const capsule = parseShadowRevisionCapsule(
      Buffer.from(entry.payload.bytesBase64, "base64"),
    );
    const version = capsule.contractVersion.endsWith("/v2") ? "v2" : "v1";
    const stage = parseShadowRevisionStage(canonicalJsonBytes({
      contractVersion: `adjustment-shadow-revision-stage/${version}`,
      input: {
        bodyBase64: capsule.bodyBase64,
        ...(version === "v2" ? { comparatorBase64: capsule.comparatorBase64 } : {}),
        metadata: capsule.metadata,
        sourceProjectionBase64: capsule.sourceProjectionBase64,
        sourceProjectionSha256: capsule.sourceProjectionSha256,
      },
      stageReceipt: entry.stageReceipt,
    }));
    // require the page's stage object to be the exact capsule member
    if (canonicalJson(stage.stageReceipt) !== canonicalJson(capsule.stageReceipt)) {
      throw new Error("adjustment shadow cold stage receipt differs");
    }
    // distinguish a genuine publication from a forever-unqualified terminal gap
    if (entry.publication.disposition === "published") {
      parseShadowRevisionPublishReceipt(
        canonicalJsonBytes(entry.publication.value),
        capsule.metadata.predictionSha256,
        capsule.comparatorSha256 ?? null,
      );
    } else if (entry.publication.disposition === "committed_unpublished_gap") {
      const gap = parseShadowRevisionGap(canonicalJsonBytes(entry.publication.value));
      // bind the exact failed publication to its admitted capsule and stage
      if (gap.input.reason !== "archive_publish_failed" ||
        gap.input.dueKey !== capsule.metadata.dueKey ||
        gap.input.registrationSha256 !== capsule.metadata.registrationSha256) {
        throw new Error("adjustment shadow cold terminal gap differs");
      }
    } else {
      throw new TypeError("adjustment shadow cold publication is invalid");
    }
  } else {
    parseRevisionStageReceipt(
      canonicalJsonBytes(entry.stageReceipt),
      entry.receipt.projectionIdentitySha256,
      entry.receipt.projectionKind,
      entry.receipt.projectionSha256,
    );
    // distinguish a genuine publication from a forever-unqualified terminal gap
    if (entry.publication.disposition === "published") {
      parseRevisionPublishReceipt(canonicalJsonBytes(entry.publication.value), entry.receipt);
    } else if (entry.publication.disposition === "committed_unpublished_gap") {
      const gap = parseRevisionGap(canonicalJsonBytes(entry.publication.value));
      // bind the exact failed publication to its admitted body and producer kind
      if (gap.input.reason !== "archive_publish_failed" ||
        gap.input.projectionIdentitySha256 !== entry.receipt.projectionIdentitySha256 ||
        gap.input.projectionSha256 !== entry.receipt.projectionSha256 ||
        gap.input.projectionKind !== entry.receipt.projectionKind) {
        throw new Error("adjustment revision cold terminal gap differs");
      }
    } else {
      throw new TypeError("adjustment revision cold publication is invalid");
    }
    // bind the additive public state only to its exact control projection
    if (entry.payload.kind === "adjustment-rain-gate-control-projection/v3") {
      validateRevisionRainControlColdAuxiliary(entry);
    }
  }
}

// verify one control projection's exact public state and durable stage receipt
function validateRevisionRainControlColdAuxiliary(entry) {
  requireExactKeys(entry.rainControlState, [
    "bytesBase64", "identitySha256", "kind", "stateSha256",
  ], "adjustment rain control cold state");
  const bytes = decodeCanonicalBase64(entry.rainControlState.bytesBase64,
    "adjustment rain control cold state");
  const state = JSON.parse(bytes.toString("utf8"));
  const projection = JSON.parse(Buffer.from(entry.payload.bytesBase64, "base64")
    .toString("utf8"));
  const stage = parseRainControlStateStageReceipt(
    canonicalJsonBytes(entry.rainControlStateStageReceipt),
    projection.stateSha256,
  );
  // crossbind raw bytes, semantic identity, projection and first availability
  if (!bytes.equals(canonicalJsonBytes(state)) ||
    entry.rainControlState.kind !== "rain-maintenance-control-state/v1" ||
    state.contractVersion !== entry.rainControlState.kind ||
    entry.rainControlState.identitySha256 !== sha256(bytes) ||
    entry.rainControlState.stateSha256 !== sha256(bytes) ||
    sha256(bytes) !== projection.stateSha256 ||
    state.ordinalArtifactSha256 !== projection.ordinalArtifactSha256 ||
    stage.stageReceiptSha256 !== projection.stateStageReceiptSha256) {
    throw new Error("adjustment rain control cold auxiliary differs");
  }
}

// verify one grouped body's exact receipts, stage, publication and successors
function validateRevisionColdBatchAuxiliary(entry) {
  const parts = revisionColdEntryParts(entry);
  if (!parts.batch) {
    throw new TypeError("adjustment revision cold batch entry is invalid");
  }
  const first = parts.receipts[0];
  const document = JSON.parse(Buffer.from(entry.payload.bytesBase64, "base64").toString("utf8"));
  // require the body geometry to match the complete receipt group
  if (!["adjustment-revision-batch-projection/v2",
    "adjustment-rain-fixed-gauge-target-projection/v1"]
    .includes(document.contractVersion) ||
    document.rows?.length !== parts.receipts.length ||
    !["actual_best_match", "target_revision"].includes(first.projectionKind)) {
    throw new Error("adjustment revision cold batch payload differs");
  }
  parseRevisionStageReceipt(
    canonicalJsonBytes(entry.stageReceipt),
    first.projectionIdentitySha256,
    first.projectionKind,
    first.projectionSha256,
  );
  // bind every successor and receipt to the same staged grouped body
  for (const [index, receipt] of parts.receipts.entries()) {
    const successor = parts.successors[index];
    validateRevisionCommitReceiptIdentity(receipt);
    requireExactKeys(successor, [
      "archiveCommitOrdinal", "contractVersion", "frontierSha256",
      "predecessorFrontierSha256", "receiptSha256",
    ], "adjustment revision cold batch successor");
    if (receipt.projectionIdentitySha256 !== first.projectionIdentitySha256 ||
      receipt.projectionSha256 !== first.projectionSha256 ||
      receipt.projectionKind !== first.projectionKind ||
      receipt.stageReceiptSha256 !== first.stageReceiptSha256 ||
      successor.archiveCommitOrdinal !== receipt.archiveCommitOrdinal ||
      successor.contractVersion !== "adjustment-revision-successor/v1" ||
      successor.frontierSha256 !== receipt.frontierSha256 ||
      successor.predecessorFrontierSha256 !== receipt.predecessorFrontierSha256 ||
      successor.receiptSha256 !== receipt.receiptSha256) {
      throw new Error("adjustment revision cold batch successor differs");
    }
  }
  requireExactKeys(entry.publication, ["disposition", "value"],
    "adjustment revision cold batch publication");
  if (entry.publication.disposition === "published") {
    parseRevisionBatchPublishReceipt(
      canonicalJsonBytes(entry.publication.value),
      parts.receipts,
    );
  } else if (entry.publication.disposition === "committed_unpublished_gap") {
    const gap = parseRevisionGap(canonicalJsonBytes(entry.publication.value));
    if (gap.input.reason !== "archive_publish_failed" ||
      gap.input.projectionIdentitySha256 !== first.projectionIdentitySha256 ||
      gap.input.projectionSha256 !== first.projectionSha256 ||
      gap.input.projectionKind !== first.projectionKind) {
      throw new Error("adjustment revision cold batch terminal gap differs");
    }
  } else {
    throw new TypeError("adjustment revision cold batch publication is invalid");
  }
}

// return the fixed global frontier genesis identity
function adjustmentRevisionGenesisFrontier() {
  return sha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
}

// validate one bounded database-frozen current-pointer snapshot
export function validateAdjustmentRevisionServingSnapshot(value) {
  requireExactKeys(value, [
    "archiveCommitOrdinal", "contractVersion", "cutoffAt", "entries", "entryCount",
    "frontierSha256", "snapshotSha256",
  ], "adjustment revision serving snapshot");
  // enforce the closed metadata-only snapshot envelope
  if (value.contractVersion !== "adjustment-revision-serving-snapshot/v1" ||
    !Array.isArray(value.entries) || value.entries.length > 4096 ||
    value.entryCount !== value.entries.length ||
    Buffer.byteLength(canonicalJson(value), "utf8") > 4 * 1024 * 1024) {
    throw new TypeError("adjustment revision serving snapshot is invalid");
  }
  requireInstant(value.cutoffAt, "adjustment revision snapshot cutoff");
  requireUint64Text(value.archiveCommitOrdinal, "adjustment revision snapshot ordinal");
  requireSha256(value.frontierSha256, "adjustment revision snapshot frontier");
  requireSha256(value.snapshotSha256, "adjustment revision snapshot identity");
  const watermark = BigInt(value.archiveCommitOrdinal);
  let priorOrdinal = 0n;
  const receiptHashes = [];
  // reject late ordinals even when they carry an old logical receipt clock
  for (const entry of value.entries) {
    requireExactKeys(entry, ["logicalReceivedAt", "receipt", "relation"],
      "adjustment revision serving snapshot entry");
    requireInstant(entry.logicalReceivedAt, "adjustment revision logical receipt time");
    validateRevisionCommitReceipt(entry.receipt);
    const ordinal = BigInt(entry.receipt.archiveCommitOrdinal);
    const relationKindValid = entry.relation === "weather_records"
      ? ["actual_best_match", "target_revision"].includes(entry.receipt.projectionKind)
      : entry.relation === "rain_adjustment_runs"
        ? entry.receipt.projectionKind === "rain_gate_input"
        : ["ecmwf_temperature_canary_runs", "forecast_anchor_records"]
            .includes(entry.relation)
          ? entry.receipt.projectionKind === "native_source"
          : false;
    // bind cutoff and watermark independently instead of trusting either clock alone
    if (Date.parse(entry.logicalReceivedAt) > Date.parse(value.cutoffAt) ||
      ordinal <= priorOrdinal || ordinal > watermark || !relationKindValid) {
      throw new Error("adjustment revision serving snapshot entry is ineligible");
    }
    priorOrdinal = ordinal;
    receiptHashes.push(entry.receipt.receiptSha256);
  }
  const expectedSha256 = sha256(Buffer.from([
    value.contractVersion,
    value.cutoffAt,
    value.archiveCommitOrdinal,
    value.frontierSha256,
    receiptHashes.join("\n"),
  ].join("\n")));
  // bind the snapshot root to every ordered current receipt identity
  if (value.snapshotSha256 !== expectedSha256) {
    throw new Error("adjustment revision serving snapshot identity differs");
  }
  return Object.freeze(value);
}

export const ADJUSTMENT_REVISION_STAGE_RECEIPT_CONTRACT_VERSION =
  "adjustment-revision-stage-receipt/v1";
export const ADJUSTMENT_REVISION_PUBLISH_RECEIPT_CONTRACT_VERSION =
  "adjustment-revision-publish-receipt/v1";
export const ADJUSTMENT_REVISION_BATCH_PUBLISH_RECEIPT_CONTRACT_VERSION =
  "adjustment-revision-batch-publish-receipt/v2";
export const ADJUSTMENT_REVISION_COLD_START_CONTRACT_VERSION =
  "adjustment-revision-cold-transfer-start/v1";
export const ADJUSTMENT_REVISION_COLD_PAGE_CONTRACT_VERSION =
  "adjustment-revision-cold-page/v1";
export const ADJUSTMENT_REVISION_COLD_CHECKPOINT_CONTRACT_VERSION =
  "adjustment-revision-cold-page-checkpoint/v1";
export const ADJUSTMENT_REVISION_CUSTODY_ACK_CONTRACT_VERSION =
  "adjustment-revision-custody-acknowledgement/v1";
export const ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION =
  "adjustment-revision-custody-acknowledgement/v2";
export const ADJUSTMENT_SHADOW_METADATA_CUSTODY_PROOF_CONTRACT_VERSION =
  "adjustment-shadow-metadata-custody-proof/v1";
export const ADJUSTMENT_REVISION_GAP_START_CONTRACT_VERSION =
  "adjustment-revision-gap-transfer-start/v1";
export const ADJUSTMENT_REVISION_GAP_PAGE_CONTRACT_VERSION =
  "adjustment-revision-gap-payload-page/v1";
export const ADJUSTMENT_REVISION_GAP_ACK_CONTRACT_VERSION =
  "adjustment-revision-gap-payload-ack/v1";
export const ADJUSTMENT_REVISION_GAP_CHECKPOINT_CONTRACT_VERSION =
  "adjustment-revision-gap-page-checkpoint/v1";
export const ADJUSTMENT_RAIN_CONTROL_STATE_GAP_CONTRACT_VERSION =
  "adjustment-rain-control-state-gap/v1";
export const ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_GAP_CONTRACT_VERSION =
  "adjustment-rain-fixed-gauge-target-gap/v1";
export const ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_CONTRACT_VERSION =
  "adjustment-rain-fixed-gauge-target-gap-status/v1";
const ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_BYTES = 4_832 * 1024;
const ADJUSTMENT_REVISION_COLD_PAGE_MAXIMUM_PAYLOADS = 2;
const ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_BYTES = 4_832 * 1024;
const ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS = 2;
const ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES = 32 * 1024;
const ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES = 3_500 * 1024;
const ADJUSTMENT_REVISION_CUSTODY_ACK_MAXIMUM_BYTES = 16 * 1024;
const ADJUSTMENT_REVISION_BATCH_MAXIMUM_RECEIPTS = 168;
const ADJUSTMENT_REVISION_BATCH_COMMIT_GROUP_CONTRACT_VERSION =
  "adjustment-revision-batch-commit-group/v2";

// retain canonical revision bodies and server receipts under the existing private root
export class AdjustmentRevisionArchiveStore {
  #now;
  #parseProjection;
  #parseRainControlState;
  #spoolMutation = Promise.resolve();
  #validateShadowCapsule;
  #validateShadowStage;
  #root;

  // require the canonical producer parser at the archive boundary
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#now = options.now ?? (() => new Date());
    this.#parseProjection = options.parseProjection;
    this.#parseRainControlState = options.parseRainControlState;
    this.#validateShadowCapsule = options.validateShadowCapsule;
    this.#validateShadowStage = options.validateShadowStage;
    // refuse a structural-only archive implementation
    if (typeof this.#parseProjection !== "function") {
      throw new TypeError("adjustment revision projection parser is required");
    }
  }

  // stage one canonical monthly rain state before any control-row scoring
  async stageRainControlState(bytesValue) {
    // require the producer's exact parser before retaining public state bytes
    if (typeof this.#parseRainControlState !== "function") {
      throw new TypeError("adjustment rain control state parser is required");
    }
    if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
      bytesValue.length > ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES) {
      throw new RangeError("adjustment rain control state bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const state = this.#parseRainControlState(bytes);
    const controlStateSha256 = sha256(bytes);
    await this.#ensureRoot();
    const path = join(this.#root, "rain-control-states",
      `sha256-${controlStateSha256}.json`);
    return await this.#withSpoolMutation(async () => {
      const existing = await readOptionalPrivateFile(
        path,
        ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
      );
      // preserve exact bytes and the first availability clock on retry
      if (existing !== null && !existing.equals(bytes)) {
        throw new Error("adjustment rain control state collision");
      }
      if (existing === null) {
        await this.#admitOnlinePayload(path, bytes, null);
        await writeExclusive(path, bytes);
      }
      const receiptPath = join(this.#root, "rain-control-state-stage-receipts",
        `sha256-${controlStateSha256}.json`);
      const receiptBytes = await readOptionalPrivateFile(receiptPath, 4096);
      // return the original durable receipt after exact byte verification
      if (receiptBytes !== null) {
        return parseRainControlStateStageReceipt(receiptBytes, controlStateSha256);
      }
      const unsigned = {
        contractVersion: "adjustment-rain-control-state-stage-receipt/v1",
        durable: true,
        durableAt: requireNowInstant(this.#now(), "adjustment rain control state durable time"),
        stateSha256: controlStateSha256,
      };
      const receipt = {
        ...unsigned,
        stageReceiptSha256: sha256(canonicalJsonBytes(unsigned)),
      };
      try {
        await writeExclusive(receiptPath, canonicalJsonBytes(receipt));
        return Object.freeze(receipt);
      } catch (error) {
        // converge a racing exact stage on its first durable clock
        if (error?.code !== "EEXIST") {
          throw error;
        }
        return parseRainControlStateStageReceipt(
          await readRegularFile(receiptPath, 4096),
          controlStateSha256,
        );
      }
    });
  }

  // permanently abandon one exact state that never acquired a projection stage
  async recordRainControlStateGap(input) {
    requireExactKeys(input, ["reason", "stageReceiptSha256", "stateSha256"],
      "adjustment rain control state gap input");
    requireSha256(input.stageReceiptSha256,
      "adjustment rain control state gap stage receipt");
    requireSha256(input.stateSha256, "adjustment rain control state gap state");
    // admit only the explicit producer-side projection failure classification
    if (input.reason !== "projection_stage_failed") {
      throw new TypeError("adjustment rain control state gap reason is invalid");
    }
    await this.#ensureRoot();
    return await this.#withSpoolMutation(async () => {
      const statePath = join(this.#root, "rain-control-states",
        `sha256-${input.stateSha256}.json`);
      const receiptPath = join(this.#root, "rain-control-state-stage-receipts",
        `sha256-${input.stateSha256}.json`);
      const stateBytes = await readRegularFile(statePath,
        ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES);
      const stateReceipt = parseRainControlStateStageReceipt(
        await readRegularFile(receiptPath, 4096), input.stateSha256);
      // crossbind the request to actual durable bytes and their first receipt
      if (sha256(stateBytes) !== input.stateSha256 ||
        stateReceipt.stageReceiptSha256 !== input.stageReceiptSha256) {
        throw new Error("adjustment rain control state gap binding differs");
      }
      const projectionNames = await readdir(join(this.#root, "revision-projections"));
      // reject foreign or excess bodies before checking the abandonment race
      if (projectionNames.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
        projectionNames.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
        throw new Error("adjustment rain control state projection census differs");
      }
      // forbid abandonment after any durable projection has acquired this state
      for (const name of projectionNames) {
        const projectionBytes = await readRegularFile(join(this.#root,
          "revision-projections", name), ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES);
        const projection = this.#parseProjection(projectionBytes);
        if (projection.contractVersion ===
          "adjustment-rain-gate-control-projection/v3" &&
          projection.stateSha256 === input.stateSha256) {
          throw new Error("adjustment rain control state already has a projection");
        }
      }
      const path = join(this.#root, "rain-control-state-gaps",
        `sha256-${input.stateSha256}.json`);
      const existing = await readOptionalPrivateFile(path, 4096);
      // retain the first exact abandonment clock on idempotent retry
      if (existing !== null) {
        return parseRainControlStateGap(existing, input);
      }
      const unsigned = {
        contractVersion: ADJUSTMENT_RAIN_CONTROL_STATE_GAP_CONTRACT_VERSION,
        gapAt: requireNowInstant(this.#now(), "adjustment rain control state gap time"),
        reason: input.reason,
        stageReceiptSha256: input.stageReceiptSha256,
        stateSha256: input.stateSha256,
      };
      const gap = { ...unsigned, gapSha256: sha256(canonicalJsonBytes(unsigned)) };
      await writeExclusive(path, canonicalJsonBytes(gap));
      return Object.freeze(gap);
    });
  }

  // persist one value-free target hour that exceeded the pre-stage body cap
  async recordRainFixedGaugeTargetGap(input) {
    validateRainFixedGaugeTargetGapKey(input, true);
    await this.#ensureRoot();
    const keySha256 = rainFixedGaugeTargetGapKeySha256(input);
    const path = join(this.#root, "rain-fixed-gauge-target-gaps",
      `sha256-${keySha256}.json`);
    return await this.#withSpoolMutation(async () => {
      const existing = await readOptionalPrivateFile(path, 4096);
      // preserve the first exact permanent disposition on retry
      if (existing !== null) {
        return parseRainFixedGaugeTargetGap(existing, input);
      }
      const names = await readdir(join(this.#root, "rain-fixed-gauge-target-gaps"));
      // bound permanent value-free metadata without silently aging it
      if (names.length >= 4096 || names.some((name) =>
        !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
        throw new RangeError("adjustment rain fixed-gauge target gap spool is full");
      }
      const unsigned = {
        contractVersion: ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_GAP_CONTRACT_VERSION,
        gapAt: requireNowInstant(this.#now(), "adjustment rain fixed-gauge target gap time"),
        logicalHourAt: input.logicalHourAt,
        logicalKeySha256: input.logicalKeySha256,
        qualificationDisposition: "forever_unqualified",
        reason: input.reason,
      };
      const gap = { ...unsigned, gapSha256: sha256(canonicalJsonBytes(unsigned)) };
      await writeExclusive(path, canonicalJsonBytes(gap));
      return Object.freeze(gap);
    });
  }

  // return only one canonical permanent disposition for the exact hour key
  async readRainFixedGaugeTargetGap(input) {
    validateRainFixedGaugeTargetGapKey(input, false);
    await this.#ensureRoot();
    const bytes = await readOptionalPrivateFile(join(this.#root,
      "rain-fixed-gauge-target-gaps",
      `sha256-${rainFixedGaugeTargetGapKeySha256(input)}.json`), 4096);
    // distinguish never-classified hours without synthesizing a gap
    if (bytes === null) {
      return Object.freeze({
        contractVersion: ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_CONTRACT_VERSION,
        state: "absent",
      });
    }
    return Object.freeze({
      contractVersion: ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_CONTRACT_VERSION,
      gap: parseRainFixedGaugeTargetGap(bytes, input),
      state: "present",
    });
  }

  // stage one canonical projection before any serving-row pointer bind
  async stageProjection(bytesValue) {
    // reject coercion and cap hostile bytes before invoking the producer parser
    if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
      bytesValue.length > ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES) {
      throw new RangeError("adjustment revision projection bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const projection = this.#parseProjection(bytes);
    const rainControl = projection.contractVersion ===
      "adjustment-rain-gate-control-projection/v3";
    const rainFixedGaugeTarget = projection.contractVersion ===
      "adjustment-rain-fixed-gauge-target-projection/v1";
    const temperatureNative = projection.contractVersion ===
      "adjustment-temperature-native-source-projection/v2";
    requireExactKeys(projection, rainControl ? [
      "contractVersion", "family", "logicalKey", "logicalReceivedAt",
      "ordinalArtifactSha256", "persistenceTarget", "projectionKind", "rows",
      "source", "stateSha256", "stateStageReceiptSha256", "storedContentSha256",
    ] : rainFixedGaugeTarget ? [
      "captureBodies", "contractVersion", "family", "logicalReceivedAt",
      "projectionKind", "rows", "validAt",
    ] : temperatureNative ? [
      "contractVersion", "family", "logicalKey", "logicalReceivedAt",
      "projectionKind", "recentErrorState", "recentErrorStateSha256", "rows",
      "source", "storedContentSha256",
    ] : [
      "contractVersion", "family", "logicalKey", "logicalReceivedAt", "projectionKind",
      "rows", "source", "storedContentSha256",
    ], "adjustment revision projection");
    // the twelve-gauge group carries one native content hash per row
    if (!rainFixedGaugeTarget) {
      requireSha256(projection.storedContentSha256,
        "adjustment revision stored content identity");
    }
    const projectionIdentitySha256 = sha256(bytes);
    const projectionSha256 = projectionIdentitySha256;
    await this.#ensureRoot();
    // require the monthly state and its first durable receipt before control rows
    if (rainControl) {
      const stateBytes = await readRegularFile(join(this.#root, "rain-control-states",
        `sha256-${projection.stateSha256}.json`), ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES);
      const state = this.#parseRainControlState(stateBytes);
      const stateReceipt = parseRainControlStateStageReceipt(
        await readRegularFile(join(this.#root, "rain-control-state-stage-receipts",
          `sha256-${projection.stateSha256}.json`), 4096),
        projection.stateSha256,
      );
      if (sha256(stateBytes) !== projection.stateSha256 ||
        state.ordinalArtifactSha256 !== projection.ordinalArtifactSha256 ||
        stateReceipt.stageReceiptSha256 !== projection.stateStageReceiptSha256) {
        throw new Error("adjustment rain control state binding differs");
      }
    }
    await this.#writeOnlinePayload(
      join(this.#root, "revision-projections", `sha256-${projectionIdentitySha256}.json`),
      bytes,
      ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES,
      rainControl
        ? async () => {
            const gap = await readOptionalPrivateFile(join(this.#root,
              "rain-control-state-gaps", `sha256-${projection.stateSha256}.json`), 4096);
            // never revive a state after its permanent abandonment became durable
            if (gap !== null) {
              throw new Error("adjustment rain control state is permanently abandoned");
            }
          }
        : null,
    );
    const receiptPath = join(this.#root, "revision-stage-receipts",
      `sha256-${projectionIdentitySha256}.json`);
    const existing = await readOptionalPrivateFile(receiptPath, 4096);

    // return the first durable stage clock unchanged on retry
    if (existing !== null) {
      return parseRevisionStageReceipt(existing, projectionIdentitySha256,
        projection.projectionKind, projectionSha256);
    }
    const unsigned = {
      contractVersion: ADJUSTMENT_REVISION_STAGE_RECEIPT_CONTRACT_VERSION,
      durable: true,
      durableAt: requireNowInstant(this.#now(), "adjustment revision durable time"),
      projectionIdentitySha256,
      projectionKind: projection.projectionKind,
      projectionSha256,
    };
    const receipt = {
      ...unsigned,
      stageReceiptSha256: sha256(canonicalJsonBytes(unsigned)),
    };
    const receiptBytes = canonicalJsonBytes(receipt);
    try {
      await writeExclusive(receiptPath, receiptBytes);
      return Object.freeze(receipt);
    } catch (error) {
      // converge a racing byte-identical stage on its first durable clock
      if (error?.code !== "EEXIST") {
        throw error;
      }
      return parseRevisionStageReceipt(
        await readRegularFile(receiptPath, 4096),
        projectionIdentitySha256,
        projection.projectionKind,
        projectionSha256,
      );
    }
  }

  // stage one canonical shadow source/body pair before its database append
  async stageShadowRevision(input) {
    const prepared = validateShadowStageInput(input);
    // require the producer's full parsers before retaining private values
    if (typeof this.#validateShadowStage !== "function") {
      throw new TypeError("adjustment shadow revision stage validator is required");
    }
    await this.#validateShadowStage({
      body: prepared.body,
      ...(prepared.comparator === null ? {} : { comparator: prepared.comparator }),
      metadata: prepared.metadata,
      sourceProjection: prepared.source,
    });
    await this.#ensureRoot();
    const path = join(this.#root, "shadow-revision-stages",
      `sha256-${prepared.metadata.predictionSha256}.json`);
    return await this.#withSpoolMutation(async () => {
      const existing = await readOptionalPrivateFile(path, 2 * 1024 * 1024);
      // return the first durable stage clock unchanged on exact retry
      if (existing !== null) {
        const stored = parseShadowRevisionStage(existing);
        if (canonicalJson(stored.input) !== canonicalJson(prepared.input)) {
          throw new Error("adjustment shadow revision stage collision");
        }
        return stored.stageReceipt;
      }
      const version = prepared.comparator === null ? "v1" : "v2";
      const unsigned = {
        contractVersion: `adjustment-shadow-stage-receipt/${version}`,
        durable: true,
        durableAt: requireNowInstant(this.#now(), "adjustment shadow stage durable time"),
        dueKey: prepared.metadata.dueKey,
        predictionBodySha256: prepared.metadata.predictionBodySha256,
        registrationSha256: prepared.metadata.registrationSha256,
        sourceProjectionSha256: prepared.input.sourceProjectionSha256,
        ...(prepared.comparator === null
          ? {}
          : { comparatorSha256: prepared.comparatorSha256 }),
      };
      const stageReceipt = {
        ...unsigned,
        stageReceiptSha256: sha256(shadowReceiptIdentityBytes(unsigned, version)),
      };
      const document = {
        contractVersion: `adjustment-shadow-revision-stage/${version}`,
        input: prepared.input,
        stageReceipt,
      };
      const bytes = canonicalJsonBytes(document);
      await this.#admitOnlinePayload(path, bytes, null);
      await writeExclusive(path, bytes);
      return Object.freeze(stageReceipt);
    });
  }

  // publish one API-admitted shadow receipt only after its exact durable stage
  async publishShadowRevision(input) {
    const comparatorTransport = input !== null && typeof input === "object" &&
      !Array.isArray(input) && Object.hasOwn(input, "comparatorBase64");
    // preserve the compact legacy v1 request while requiring full v2 value binding
    if (comparatorTransport) {
      requireExactKeys(input, [
        "bodyBase64", "comparatorBase64", "metadata", "revisionReceipt",
        "predictionCommittedAt", "sourceProjectionBase64", "sourceProjectionSha256",
        "stageReceipt",
      ], "adjustment shadow revision v2 publish input");
    } else {
      requireExactKeys(input, [
        "metadata", "revisionReceipt", "sourceProjectionSha256", "stageReceiptSha256",
      ], "adjustment shadow revision publish input");
    }
    validateRevisionCommitReceiptIdentity(input.revisionReceipt);
    // accept only the distinct server-issued shadow grammar
    if (input.revisionReceipt.projectionKind !== "shadow_prediction") {
      throw new TypeError("adjustment shadow revision receipt kind is invalid");
    }
    requireSha256(input.sourceProjectionSha256,
      "adjustment shadow revision publish source identity");
    requireSha256(comparatorTransport
      ? input.stageReceipt.stageReceiptSha256
      : input.stageReceiptSha256,
    "adjustment shadow revision publish stage identity");
    requireShadowMetadata(input.metadata);
    // bind the exact database append clock between issuance and archive commitment
    if (comparatorTransport) {
      requireInstant(input.predictionCommittedAt,
        "adjustment shadow prediction commit time");
      if (Date.parse(input.predictionCommittedAt) < Date.parse(input.metadata.issuedAt) ||
        Date.parse(input.predictionCommittedAt) >
          Date.parse(input.revisionReceipt.archiveCommittedAt)) {
        throw new Error("adjustment shadow prediction commit time differs");
      }
    }
    const publishedStage = comparatorTransport ? validateShadowStageInput({
      bodyBase64: input.bodyBase64,
      comparatorBase64: input.comparatorBase64,
      metadata: input.metadata,
      sourceProjectionBase64: input.sourceProjectionBase64,
      sourceProjectionSha256: input.sourceProjectionSha256,
    }) : null;
    await this.#ensureRoot();
    const permanentGaps = (await readStoredShadowRevisionGaps(this.#root)).filter(
      // bind any permanent classification to this exact scheduled admission
      (gap) => gap.input.dueKey === input.metadata.dueKey &&
        gap.input.registrationSha256 === input.metadata.registrationSha256,
    );
    // never repair or relabel a permanently unqualified unchanged retry
    if (permanentGaps.length > 0) {
      throw new Error("adjustment shadow revision is permanently unqualified");
    }
    const stagePath = join(this.#root, "shadow-revision-stages",
      `sha256-${input.metadata.predictionSha256}.json`);
    const staged = parseShadowRevisionStage(
      await readRegularFile(stagePath, 2 * 1024 * 1024),
    );
    // require the compact API admission to name the exact staged private bytes
    if ((publishedStage !== null &&
        canonicalJson(publishedStage.input) !== canonicalJson(staged.input)) ||
      canonicalJson(input.metadata) !== canonicalJson(staged.input.metadata) ||
      input.sourceProjectionSha256 !== staged.input.sourceProjectionSha256 ||
      (comparatorTransport
        ? canonicalJson(input.stageReceipt) !== canonicalJson(staged.stageReceipt)
        : input.stageReceiptSha256 !== staged.stageReceipt.stageReceiptSha256) ||
      input.revisionReceipt.projectionIdentitySha256 !== input.metadata.sourceReceiptSha256 ||
      input.revisionReceipt.projectionSha256 !== input.metadata.inputSha256 ||
      input.revisionReceipt.stageReceiptSha256 !== staged.stageReceipt.stageReceiptSha256) {
      throw new Error("adjustment shadow revision publish differs from stage");
    }
    const capsule = {
      bodyBase64: staged.input.bodyBase64,
      ...(comparatorTransport ? {
        comparatorBase64: staged.input.comparatorBase64,
        comparatorSha256: publishedStage.comparatorSha256,
      } : {}),
      contractVersion: comparatorTransport
        ? "adjustment-shadow-revision-capsule/v2"
        : "adjustment-shadow-revision-capsule/v1",
      metadata: staged.input.metadata,
      ...(comparatorTransport
        ? { predictionCommittedAt: input.predictionCommittedAt }
        : {}),
      revisionReceipt: input.revisionReceipt,
      sourceProjectionBase64: staged.input.sourceProjectionBase64,
      sourceProjectionSha256: staged.input.sourceProjectionSha256,
      stageReceipt: staged.stageReceipt,
    };
    await this.persistShadowRevisionCapsule(canonicalJsonBytes(capsule));
    const receiptPath = join(this.#root, "shadow-revision-publish-receipts",
      `sha256-${input.metadata.predictionSha256}.json`);
    const existing = await readOptionalPrivateFile(receiptPath, 4096);
    // preserve the first durable publication clock on retry
    if (existing !== null) {
      return parseShadowRevisionPublishReceipt(
        existing,
        input.metadata.predictionSha256,
        publishedStage?.comparatorSha256 ?? null,
      );
    }
    const unsigned = {
      committed: true,
      committedAt: requireNowInstant(this.#now(), "adjustment shadow commit time"),
      contractVersion: comparatorTransport
        ? "adjustment-shadow-publish-receipt/v2"
        : "adjustment-shadow-publish-receipt/v1",
      predictionSha256: input.metadata.predictionSha256,
      ...(comparatorTransport
        ? { comparatorSha256: publishedStage.comparatorSha256 }
        : {}),
    };
    const receipt = {
      ...unsigned,
      publishReceiptSha256: sha256(shadowReceiptIdentityBytes(
        unsigned,
        comparatorTransport ? "v2" : "v1",
      )),
    };
    try {
      await writeExclusive(receiptPath, canonicalJsonBytes(receipt));
      return Object.freeze(receipt);
    } catch (error) {
      // converge a racing byte-identical publication on its first durable clock
      if (error?.code !== "EEXIST") {
        throw error;
      }
      return parseShadowRevisionPublishReceipt(
        await readRegularFile(receiptPath, 4096),
        input.metadata.predictionSha256,
        publishedStage?.comparatorSha256 ?? null,
      );
    }
  }

  // retain one bounded value-free shadow failure without source or body bytes
  async recordShadowGap(input) {
    requireExactKeys(input, ["dueKey", "family", "reason", "registrationSha256"],
      "adjustment shadow gap input");
    // accept only fixed scheduler identities and categorical failure classes
    if (!/^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u
      .test(input.dueKey) || !["rain", "temperature", "wind"].includes(input.family) ||
      !["candidate_unavailable", "comparator_unavailable", "source_incomplete", "archive_stage_failed",
        "database_append_failed", "database_admission_failed", "archive_publish_failed"]
        .includes(input.reason)) {
      throw new TypeError("adjustment shadow gap is invalid");
    }
    requireSha256(input.registrationSha256, "adjustment shadow gap registration");
    await this.#ensureRoot();
    const identitySha256 = sha256(canonicalJsonBytes({
      dueKey: input.dueKey,
      registrationSha256: input.registrationSha256,
    }));
    return await this.#withSpoolMutation(async () => {
      const path = join(this.#root, "shadow-revision-gaps",
        `sha256-${identitySha256}.json`);
      const existing = await readOptionalPrivateFile(path, 4096);
      // preserve one permanent first classification for each scheduled identity
      if (existing !== null) {
        const value = JSON.parse(existing.toString("utf8"));
        if (!existing.equals(canonicalJsonBytes(value)) ||
          canonicalJson(value.input) !== canonicalJson(input)) {
          throw new Error("adjustment shadow gap collision");
        }
        return Object.freeze(value);
      }
      const names = await readdir(join(this.#root, "shadow-revision-gaps"));
      // cap value-free failures within the reviewed fourteen-day metadata budget
      if (names.length >= 4096 || names.some((name) =>
        !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
        throw new RangeError("adjustment shadow gap spool is full");
      }
      const value = {
        contractVersion: "adjustment-shadow-gap/v1",
        input,
        qualificationDisposition: "forever_unqualified",
        recordedAt: requireNowInstant(this.#now(), "adjustment shadow gap time"),
      };
      const durable = { ...value, gapSha256: sha256(canonicalJsonBytes(value)) };
      await writeExclusive(path, canonicalJsonBytes(durable));
      return Object.freeze(durable);
    });
  }

  // publish one database-authenticated revision receipt after its durable stage
  async publishRevision(input) {
    // select only the additive grouped weather publish grammar
    if (input !== null && typeof input === "object" && !Array.isArray(input) &&
      Object.hasOwn(input, "revisionReceipts")) {
      return await this.#publishRevisionBatch(input);
    }
    requireExactKeys(input, ["revisionReceipt", "stageReceipt"],
      "adjustment revision publish input");
    validateRevisionCommitReceipt(input.revisionReceipt);
    const revision = input.revisionReceipt;
    const stage = input.stageReceipt;
    const stagePath = join(this.#root, "revision-stage-receipts",
      `sha256-${revision.projectionIdentitySha256}.json`);
    await this.#ensureRoot();
    const permanentGaps = (await readStoredRevisionGaps(this.#root)).filter(
      // bind any permanent classification to this exact admitted body
      (gap) => gap.input.projectionIdentitySha256 ===
        revision.projectionIdentitySha256 &&
        gap.input.projectionKind === revision.projectionKind,
    );
    // never repair or relabel a permanently unqualified unchanged retry
    if (permanentGaps.length > 0) {
      throw new Error("adjustment revision is permanently unqualified");
    }
    const storedStage = parseRevisionStageReceipt(
      await readRegularFile(stagePath, 4096),
      revision.projectionIdentitySha256,
      revision.projectionKind,
      revision.projectionSha256,
    );
    // require the exact durable receipt supplied to the database binder
    if (canonicalJson(stage) !== canonicalJson(storedStage) ||
      revision.stageReceiptSha256 !== storedStage.stageReceiptSha256 ||
      revision.projectionSha256 !== revision.projectionIdentitySha256) {
      throw new Error("adjustment revision publish stage differs");
    }
    // independently authenticate the database receipt before cold publication
    validateRevisionCommitReceiptIdentity(revision);
    await this.persistRevisionCommitReceipt(revision);
    const publishPath = join(this.#root, "revision-publish-receipts",
      `sha256-${revision.projectionIdentitySha256}.json`);
    const existing = await readOptionalPrivateFile(publishPath, 4096);

    // return the first durable publication clock unchanged on retry
    if (existing !== null) {
      return parseRevisionPublishReceipt(existing, revision);
    }
    const unsigned = {
      committed: true,
      committedAt: requireNowInstant(this.#now(), "adjustment revision commit time"),
      contractVersion: ADJUSTMENT_REVISION_PUBLISH_RECEIPT_CONTRACT_VERSION,
      projectionIdentitySha256: revision.projectionIdentitySha256,
      projectionKind: revision.projectionKind,
      projectionSha256: revision.projectionSha256,
      revisionReceiptSha256: revision.receiptSha256,
    };
    const receipt = {
      ...unsigned,
      publishReceiptSha256: sha256(canonicalJsonBytes(unsigned)),
    };
    try {
      await writeExclusive(publishPath, canonicalJsonBytes(receipt));
      return Object.freeze(receipt);
    } catch (error) {
      // converge a racing byte-identical publication on its first durable clock
      if (error?.code !== "EEXIST") {
        throw error;
      }
      return parseRevisionPublishReceipt(
        await readRegularFile(publishPath, 4096),
        revision,
      );
    }
  }

  // publish one complete grouped weather body and all of its ordered receipts
  async #publishRevisionBatch(input) {
    requireExactKeys(input, ["revisionReceipts", "stageReceipt"],
      "adjustment revision batch publish input");
    // retain one bounded nonempty receipt group only
    if (!Array.isArray(input.revisionReceipts) || input.revisionReceipts.length < 1 ||
      input.revisionReceipts.length > ADJUSTMENT_REVISION_BATCH_MAXIMUM_RECEIPTS) {
      throw new TypeError("adjustment revision batch receipts are invalid");
    }
    const receipts = input.revisionReceipts.map(
      // authenticate every server-issued receipt before using its chain coordinates
      (receipt) => validateRevisionCommitReceiptIdentity(receipt),
    );
    const first = receipts[0];
    await this.#ensureRoot();
    const stagePath = join(this.#root, "revision-stage-receipts",
      `sha256-${first.projectionIdentitySha256}.json`);
    const storedStage = parseRevisionStageReceipt(
      await readRegularFile(stagePath, 4096),
      first.projectionIdentitySha256,
      first.projectionKind,
      first.projectionSha256,
    );
    const projectionBytes = await readRegularFile(
      join(this.#root, "revision-projections",
        `sha256-${first.projectionIdentitySha256}.json`),
      ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES,
    );
    const projection = this.#parseProjection(projectionBytes);
    // restrict batching to the two frozen grouped weather projection contracts
    if (!["adjustment-revision-batch-projection/v2",
      "adjustment-rain-fixed-gauge-target-projection/v1"]
      .includes(projection.contractVersion) ||
      !["actual_best_match", "target_revision"].includes(projection.projectionKind) ||
      projection.projectionKind !== first.projectionKind ||
      projection.rows.length !== receipts.length ||
      canonicalJson(input.stageReceipt) !== canonicalJson(storedStage)) {
      throw new Error("adjustment revision batch projection differs");
    }
    // bind every receipt to the same staged body and one direct global chain
    for (const [index, receipt] of receipts.entries()) {
      const predecessor = receipts[index - 1];
      if (receipt.projectionIdentitySha256 !== first.projectionIdentitySha256 ||
        receipt.projectionSha256 !== first.projectionSha256 ||
        receipt.projectionKind !== first.projectionKind ||
        receipt.stageReceiptSha256 !== storedStage.stageReceiptSha256 ||
        (predecessor !== undefined &&
          (receipt.predecessorFrontierSha256 !== predecessor.frontierSha256 ||
            BigInt(receipt.archiveCommitOrdinal) !==
              BigInt(predecessor.archiveCommitOrdinal) + 1n))) {
        throw new Error("adjustment revision batch receipt chain differs");
      }
    }
    const permanentGaps = (await readStoredRevisionGaps(this.#root)).filter(
      // bind any permanent classification to this exact grouped body
      (gap) => gap.input.projectionIdentitySha256 === first.projectionIdentitySha256 &&
        gap.input.projectionKind === first.projectionKind,
    );
    if (permanentGaps.length > 0) {
      throw new Error("adjustment revision is permanently unqualified");
    }
    return await this.#withSpoolMutation(async () => {
      await this.#persistRevisionCommitGroup(receipts);
      const publishPath = join(this.#root, "revision-publish-receipts",
        `sha256-${first.projectionIdentitySha256}.json`);
      const existing = await readOptionalPrivateFile(publishPath, 32 * 1024);
      // preserve the first complete grouped publication on retry
      if (existing !== null) {
        return parseRevisionBatchPublishReceipt(existing, receipts);
      }
      const unsigned = {
        committed: true,
        committedAt: requireNowInstant(this.#now(), "adjustment revision batch commit time"),
        contractVersion: ADJUSTMENT_REVISION_BATCH_PUBLISH_RECEIPT_CONTRACT_VERSION,
        projectionIdentitySha256: first.projectionIdentitySha256,
        projectionKind: first.projectionKind,
        projectionSha256: first.projectionSha256,
        revisionReceiptSha256s: receipts.map(
          // preserve exact database receipt order in the publication root
          (receipt) => receipt.receiptSha256,
        ),
      };
      const publication = {
        ...unsigned,
        publishReceiptSha256: sha256(canonicalJsonBytes(unsigned)),
      };
      try {
        await writeExclusive(publishPath, canonicalJsonBytes(publication));
        return Object.freeze(publication);
      } catch (error) {
        // converge only a racing byte-identical grouped publication
        if (error?.code !== "EEXIST") {
          throw error;
        }
        return parseRevisionBatchPublishReceipt(
          await readRegularFile(publishPath, 32 * 1024),
          receipts,
        );
      }
    });
  }

  // persist one grouped body chain in one bounded metadata file
  async #persistRevisionCommitGroup(receipts) {
    const successors = receipts.map(
      // retain every direct frontier successor without one file per row
      (revision) => ({
        archiveCommitOrdinal: revision.archiveCommitOrdinal,
        contractVersion: "adjustment-revision-successor/v1",
        frontierSha256: revision.frontierSha256,
        predecessorFrontierSha256: revision.predecessorFrontierSha256,
        receiptSha256: revision.receiptSha256,
      }),
    );
    const group = validateRevisionBatchCommitGroup({
      contractVersion: ADJUSTMENT_REVISION_BATCH_COMMIT_GROUP_CONTRACT_VERSION,
      projectionIdentitySha256: receipts[0].projectionIdentitySha256,
      receipts,
      successors,
    });
    await writeOrVerifyImmutable(
      join(this.#root, "revision-commit-receipts",
        `batch-sha256-${group.projectionIdentitySha256}.json`),
      canonicalJsonBytes(group),
      256 * 1024,
      writeExclusive,
    );
    return group;
  }

  // persist one authenticated server receipt into the cold successor chain
  async persistRevisionCommitReceipt(revision) {
    validateRevisionCommitReceiptIdentity(revision);
    await this.#ensureRoot();
    const receiptBytes = canonicalJsonBytes(revision);
    await writeOrVerifyImmutable(
      join(this.#root, "revision-commit-receipts",
        `sha256-${revision.frontierSha256}.json`),
      receiptBytes,
      4096,
      writeExclusive,
    );
    const successor = {
      archiveCommitOrdinal: revision.archiveCommitOrdinal,
      contractVersion: "adjustment-revision-successor/v1",
      frontierSha256: revision.frontierSha256,
      predecessorFrontierSha256: revision.predecessorFrontierSha256,
      receiptSha256: revision.receiptSha256,
    };
    await writeOrVerifyImmutable(
      join(this.#root, "revision-frontier-successors",
        `sha256-${revision.predecessorFrontierSha256}.json`),
      canonicalJsonBytes(successor),
      4096,
      writeExclusive,
    );
    return Object.freeze(revision);
  }

  // persist one parser-verified shadow capsule and its distinct receipt grammar
  async persistShadowRevisionCapsule(bytesValue) {
    // reject implicit byte coercion at the private capsule boundary
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("adjustment shadow revision capsule must be a Buffer");
    }
    const bytes = Buffer.from(bytesValue);
    // require the producer's full source and body parsers at this trust boundary
    if (typeof this.#validateShadowCapsule !== "function") {
      throw new TypeError("adjustment shadow revision capsule validator is required");
    }
    const capsule = parseShadowRevisionCapsule(bytes);
    await this.#validateShadowCapsule(capsule, bytes);
    const stagePath = join(this.#root, "shadow-revision-stages",
      `sha256-${capsule.metadata.predictionSha256}.json`);
    const capsulePath = join(this.#root, "shadow-revision-capsules",
      `sha256-${capsule.revisionReceipt.receiptSha256}.json`);
    await this.#withSpoolMutation(async () => {
      await this.#admitOnlinePayload(capsulePath, bytes, stagePath);
      await this.persistRevisionCommitReceipt(capsule.revisionReceipt);
      await writeOrVerifyImmutable(capsulePath, bytes, 2 * 1024 * 1024, writeExclusive);
      // retire only the redundant online stage after the complete capsule is durable
      await unlink(stagePath).catch(
        // accept only a retry after the stage was already removed
        (error) => error?.code === "ENOENT" ? undefined : Promise.reject(error),
      );
      await fsyncDirectory(dirname(stagePath));
    });
    return Object.freeze(capsule);
  }

  // record one permanent categorical projection failure without value bytes
  async recordGap(input) {
    requireExactKeys(input, [
      "logicalKeySha256", "projectionIdentitySha256", "projectionKind",
      "projectionSha256", "reason",
    ], "adjustment revision gap input");
    requireSha256(input.logicalKeySha256, "adjustment revision gap logical key identity");
    // require the closed projection and failure classes only
    if (!["actual_best_match", "native_source", "rain_gate_input", "target_revision"]
      .includes(input.projectionKind) ||
      !["archive_stage_failed", "database_bind_failed", "database_admission_failed",
        "archive_publish_failed"].includes(input.reason) ||
      ((input.projectionSha256 === null) !== (input.projectionIdentitySha256 === null)) ||
      (input.projectionSha256 !== null &&
        (input.projectionSha256 !== input.projectionIdentitySha256 ||
          !/^[a-f0-9]{64}$/u.test(input.projectionSha256)))) {
      throw new TypeError("adjustment revision gap is invalid");
    }
    await this.#ensureRoot();
    const gapKeySha256 = sha256(canonicalJsonBytes({
      logicalKeySha256: input.logicalKeySha256,
      projectionIdentitySha256: input.projectionIdentitySha256,
    }));
    const path = join(this.#root, "revision-gaps",
      `sha256-${gapKeySha256}.json`);
    return await this.#withSpoolMutation(async () => {
      const existing = await readOptionalPrivateFile(path, 4096);

      // keep the first permanent classification authoritative on retry
      if (existing !== null) {
        const value = JSON.parse(existing.toString("utf8"));
        // reject mutation or relabelling of an already permanent gap
        if (!existing.equals(canonicalJsonBytes(value)) ||
          canonicalJson(value.input) !== canonicalJson(input)) {
          throw new Error("adjustment revision gap collision");
        }
        return Object.freeze(value);
      }
      const names = await readdir(join(this.#root, "revision-gaps"));
      // cap every one-file gap representation under the reviewed metadata budget
      if (names.length >= 4096 || names.some((name) =>
        !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
        throw new RangeError("adjustment revision gap spool is full");
      }
      const value = {
        contractVersion: "adjustment-revision-gap/v1",
        input,
        qualificationDisposition: "forever_unqualified",
        recordedAt: requireNowInstant(this.#now(), "adjustment revision gap time"),
      };
      const durable = {
        ...value,
        gapSha256: sha256(canonicalJsonBytes(value)),
      };
      await writeExclusive(path, canonicalJsonBytes(durable));
      return Object.freeze(durable);
    });
  }

  // create only fixed private revision archive children
  async #ensureRoot() {
    await requirePrivateDirectory(this.#root);
    // retain content-addressed bodies and receipts in separate fixed directories
    for (const directory of ["revision-projections", "revision-stage-receipts",
      "revision-commit-receipts", "revision-frontier-successors",
      "revision-publish-receipts", "revision-gaps", "shadow-revision-capsules",
      "shadow-revision-stages", "shadow-revision-publish-receipts",
      "shadow-revision-gaps", "revision-custody-acknowledgements",
      "revision-gap-transfer", "revision-spool-locks", "rain-control-states",
      "rain-control-state-stage-receipts", "rain-control-state-gaps",
      "rain-fixed-gauge-target-gaps"]) {
      await ensurePrivateDirectory(join(this.#root, directory));
    }
  }

  // serialize one online payload census and immutable publication
  async #writeOnlinePayload(path, bytes, maximumBytes, beforeWrite = null) {
    return await this.#withSpoolMutation(async () => {
      // execute one optional state/gap race check under the same filesystem lock
      if (beforeWrite !== null) {
        await beforeWrite();
      }
      await this.#admitOnlinePayload(path, bytes, null);
      return await writeOrVerifyImmutable(path, bytes, maximumBytes, writeExclusive);
    });
  }

  // admit one new payload without exceeding two slots or the cycle byte ceiling
  async #admitOnlinePayload(path, bytes, replacementPath) {
    const existing = await readOptionalPrivateFile(path,
      ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES);
    // preserve byte-identical retries without consuming another slot
    if (existing !== null) {
      if (!existing.equals(bytes)) {
        throw new Error("adjustment revision online payload collision");
      }
      return;
    }
    let slotCount = 0;
    let byteCount = 0;
    // census only the four fixed value-bearing spool directories
    for (const [directory, maximumBytes, pattern] of [
      ["revision-projections", ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES, /^sha256-[a-f0-9]{64}\.json$/u],
      ["shadow-revision-capsules", 2 * 1024 * 1024, /^sha256-[a-f0-9]{64}\.json$/u],
      ["shadow-revision-stages", 2 * 1024 * 1024, /^sha256-[a-f0-9]{64}\.json$/u],
      ["rain-control-states", ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES,
        /^sha256-[a-f0-9]{64}\.json$/u],
    ]) {
      const directoryPath = join(this.#root, directory);
      const names = await readdir(directoryPath);
      // reject foreign spool entries instead of hiding them from accounting
      if (names.some((name) => !pattern.test(name))) {
        throw new Error("adjustment revision online payload census differs");
      }
      // charge every retained regular payload before admitting another
      for (const name of names) {
        const memberPath = join(directoryPath, name);
        const member = await readRegularFile(memberPath, maximumBytes);
        // omit only the exact stage replaced by its complete capsule
        if (replacementPath !== null && memberPath === replacementPath) {
          continue;
        }
        slotCount += 1;
        byteCount += member.length;
      }
    }
    // refuse before mutation so callers can record one permanent fail-open gap
    if (slotCount + 1 > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
      byteCount + bytes.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_BYTES) {
      const error = new RangeError("adjustment revision online spool is full");
      error.code = "adjustment_revision_spool_refused";
      throw error;
    }
  }

  // serialize all payload admissions within this process without hidden queues
  async #withSpoolMutation(operation) {
    const previous = this.#spoolMutation;
    let release;
    this.#spoolMutation = new Promise(
      // retain one resolver for the next bounded mutation
      (resolveRelease) => { release = resolveRelease; },
    );
    await previous;
    try {
      return await withRevisionSpoolLock(this.#root, operation);
    } finally {
      release();
    }
  }
}

// create one route-neutral adapter for fixed shadow and revision archive endpoints
export function createAdjustmentRevisionArchiveHandler(options = {}) {
  const store = options.store;
  // require the concrete owner-private store instead of accepting duck typing
  if (!(store instanceof AdjustmentRevisionArchiveStore)) {
    throw new TypeError("adjustment revision archive store is required");
  }
  return async function handleAdjustmentRevisionArchive(path, input) {
    // stage one canonical monthly rain control state before control-row capture
    if (path === "/internal/adjustment-maintenance/rain-control-state/stage") {
      requireExactKeys(input, ["stateBase64"], "adjustment rain control state request");
      const bytes = decodeCanonicalBase64(
        input.stateBase64,
        "adjustment rain control state",
      );
      return store.stageRainControlState(bytes);
    }
    // persist explicit state abandonment only when no projection acquired it
    if (path === "/internal/adjustment-maintenance/rain-control-state/gap") {
      return store.recordRainControlStateGap(input);
    }
    // retain one value-free oversized fixed-gauge hour before body staging
    if (path === "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap") {
      return store.recordRainFixedGaugeTargetGap(input);
    }
    // expose only the permanent disposition for one exact fixed-gauge hour
    if (path === "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status") {
      return store.readRainFixedGaugeTargetGap(input);
    }
    // stage one shadow source/body pair before its compact database append
    if (path === "/internal/adjustment-maintenance/archive/stage") {
      return store.stageShadowRevision(input);
    }
    // publish one admitted shadow receipt and its complete private capsule
    if (path === "/internal/adjustment-maintenance/archive/publish") {
      return store.publishShadowRevision(input);
    }
    // retain one value-free categorical shadow failure
    if (path === "/internal/adjustment-maintenance/archive/gap") {
      return store.recordShadowGap(input);
    }
    // dispatch only the three fixed canonical revision route suffixes
    if (path === "/internal/adjustment-maintenance/revision/stage") {
      requireExactKeys(input, ["projectionBase64"], "adjustment revision stage request");
      const bytes = decodeCanonicalBase64(
        input.projectionBase64,
        "adjustment revision projection",
      );
      return store.stageProjection(bytes);
    }
    // authenticate and publish only an exact database receipt pair
    if (path === "/internal/adjustment-maintenance/revision/publish") {
      // retain exact v1 or additive grouped-v2 keys without caller extensions
      if (input !== null && typeof input === "object" && !Array.isArray(input) &&
        Object.hasOwn(input, "revisionReceipts")) {
        requireExactKeys(input, ["revisionReceipts", "stageReceipt"],
          "adjustment revision batch publish request");
      } else {
        requireExactKeys(input, ["revisionReceipt", "stageReceipt"],
          "adjustment revision publish request");
      }
      return store.publishRevision(input);
    }
    // retain one value-free permanent categorical failure
    if (path === "/internal/adjustment-maintenance/revision/gap") {
      return store.recordGap(input);
    }
    throw new RangeError("adjustment revision archive route is invalid");
  };
}

const ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES = Object.freeze([
  "actual_best_match", "artifact", "candidate", "comparator", "native_source",
  "rain_gate_input", "shadow_body", "shadow_source", "target", "target_revision",
]);

// validate one custody-only permanent unsupported terminal proof
export function validateAdjustmentUnsupportedTerminalProofV1(value) {
  requireExactKeys(value, [
    "actionSha256", "archiveCommitOrdinal", "burnSha256", "candidateReportSha256",
    "captureEpochWitnessSha256", "confirmationAccessSha256",
    "confirmationChunkCount", "contractVersion", "controlSha256", "controlVersion",
    "custodyCheckpointSha256", "dueKey", "eligiblePredictionSetSha256",
    "expectedKeySetSha256", "family", "finalizedAt", "frontierSha256",
    "fullGraphVerifiedAt", "fullMemberRootSha256", "graphManifestSha256",
    "inputClasses", "lifecycleLedgerRootSha256", "missingClassNames",
    "missingKeyCount", "missingKeySetSha256", "pageSha256", "policyReportSha256",
    "predecessorProofSha256", "registrationSha256", "requiredInputRootSha256",
    "sequence", "sourceCommit", "unsupportedReason", "workstationJournalHeadSha256",
  ], "adjustment unsupported terminal proof");
  const due = /^confirmation\/(temperature|wind|rain)\/([a-f0-9]{64})$/u
    .exec(value.dueKey);
  // keep permanent unsupported custody disjoint from C, T, F and action authority
  if (value.contractVersion !== ADJUSTMENT_UNSUPPORTED_TERMINAL_PROOF_CONTRACT_VERSION ||
    due === null || due[1] !== value.family ||
    value.confirmationChunkCount !== (value.family === "rain" ? 24 : 27) ||
    !["permanent_capture_gap", "permanent_source_gap", "permanent_target_gap"]
      .includes(value.unsupportedReason) ||
    !/^[1-9][0-9]{0,5}$/u.test(value.controlVersion) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("adjustment unsupported terminal proof contract is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal,
    "adjustment unsupported terminal archive ordinal");
  requireUint64Text(value.sequence, "adjustment unsupported terminal sequence");
  requireInstant(value.finalizedAt, "adjustment unsupported terminal finalizedAt");
  requireInstant(value.fullGraphVerifiedAt,
    "adjustment unsupported terminal fullGraphVerifiedAt");
  // bind every terminal report, graph, custody and population identity
  for (const field of [
    "actionSha256", "burnSha256", "candidateReportSha256",
    "captureEpochWitnessSha256", "confirmationAccessSha256", "controlSha256",
    "custodyCheckpointSha256", "eligiblePredictionSetSha256",
    "expectedKeySetSha256", "frontierSha256", "fullMemberRootSha256",
    "graphManifestSha256", "lifecycleLedgerRootSha256", "missingKeySetSha256",
    "pageSha256", "policyReportSha256", "registrationSha256",
    "requiredInputRootSha256", "workstationJournalHeadSha256",
  ]) {
    requireSha256(value[field], `adjustment unsupported terminal ${field}`);
  }
  // permit only the first proof to omit its direct predecessor
  if (value.predecessorProofSha256 !== null) {
    requireSha256(value.predecessorProofSha256,
      "adjustment unsupported terminal predecessorProofSha256");
  }
  if ((value.sequence === "0") !== (value.predecessorProofSha256 === null) ||
    Date.parse(value.finalizedAt) < Date.parse(value.fullGraphVerifiedAt) ||
    !Number.isSafeInteger(value.missingKeyCount) || value.missingKeyCount < 1 ||
    value.missingKeyCount > 9_500_000) {
    throw new TypeError("adjustment unsupported terminal predecessor is invalid");
  }
  requireExactKeys(value.inputClasses, ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES,
    "adjustment unsupported terminal input classes");
  const emptyRootSha256 = sha256(canonicalJsonBytes([]));
  const missingClassNames = [];
  // validate every exact sorted-identity population without requiring completion
  for (const name of ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES) {
    const entry = value.inputClasses[name];
    requireExactKeys(entry, ["count", "rootSha256"],
      "adjustment unsupported terminal input class");
    requireSha256(entry.rootSha256,
      `adjustment unsupported terminal ${name} rootSha256`);
    // preserve the unique empty representation and bounded cardinality
    if (!Number.isSafeInteger(entry.count) || entry.count < 0 ||
      entry.count > 9_500_000 ||
      (entry.count === 0 && entry.rootSha256 !== emptyRootSha256)) {
      throw new TypeError("adjustment unsupported terminal population is invalid");
    }
    // omit the intentionally absent non-rain gate class from missing diagnostics
    if (entry.count === 0 && (value.family === "rain" || name !== "rain_gate_input")) {
      missingClassNames.push(name);
    }
  }
  // require retained candidate/artifact evidence and the exact missing-class projection
  if (value.inputClasses.candidate.count !== 1 ||
    value.inputClasses.candidate.rootSha256 !== sha256(canonicalJsonBytes([due[2]])) ||
    value.inputClasses.artifact.count < 1 ||
    canonicalJson(value.missingClassNames) !== canonicalJson(missingClassNames) ||
    value.requiredInputRootSha256 !== sha256(canonicalJsonBytes(value.inputClasses))) {
    throw new Error("adjustment unsupported terminal input population differs");
  }
  return Object.freeze(value);
}

// validate one typed unsupported terminal installation response
export function validateAdjustmentUnsupportedTerminalProofInstallation(value) {
  requireExactKeys(value, ["contractVersion", "proofSha256", "state"],
    "adjustment unsupported terminal installation");
  requireSha256(value.proofSha256,
    "adjustment unsupported terminal installation proofSha256");
  // admit only one custody-only successful installation state
  if (value.contractVersion !==
      "adjustment-maintenance-unsupported-terminal-proof-installation/v1" ||
    value.state !== "installed") {
    throw new TypeError("adjustment unsupported terminal installation is invalid");
  }
  return Object.freeze(value);
}

// validate one nullable current unsupported terminal projection
export function validateAdjustmentUnsupportedTerminalProofCurrent(value) {
  // preserve the empty create-once state without inventing terminal proof
  if (value === null) {
    return null;
  }
  requireExactKeys(value, ["proof", "proofSha256"],
    "adjustment unsupported terminal current");
  validateAdjustmentUnsupportedTerminalProofV1(value.proof);
  requireSha256(value.proofSha256,
    "adjustment unsupported terminal current proofSha256");
  // bind the response identity to the exact canonical proof bytes
  if (sha256(canonicalJsonBytes(value.proof)) !== value.proofSha256) {
    throw new Error("adjustment unsupported terminal current identity differs");
  }
  return Object.freeze(value);
}

// validate one typed future-only seal installation response
export function validateAdjustmentFutureOnlyInputSealInstallation(value) {
  requireExactKeys(value, ["contractVersion", "sealSha256", "state"],
    "future-only input seal installation");
  requireSha256(value.sealSha256, "future-only input seal installation sealSha256");
  // admit only the one successful immutable installation state
  if (value.contractVersion !== "adjustment-future-only-input-seal-installation/v2" ||
    value.state !== "installed") {
    throw new TypeError("future-only input seal installation is invalid");
  }
  return Object.freeze(value);
}

// validate one nullable exact current future-only seal projection
export function validateAdjustmentFutureOnlyInputSealCurrent(value) {
  // preserve exact genesis absence without inventing predecessor sequence
  if (value === null) {
    return null;
  }
  requireExactKeys(value, ["seal", "sealSha256"], "future-only input seal current");
  validateAdjustmentFutureOnlyInputSeal(value.seal);
  requireSha256(value.sealSha256, "future-only input seal current sealSha256");
  // bind the returned identity to the complete canonical seal document
  if (sha256(canonicalJsonBytes(value.seal)) !== value.sealSha256) {
    throw new Error("future-only input seal current identity differs");
  }
  return Object.freeze(value);
}

// validate one typed future-only transferred-anchor response
export function validateAdjustmentMaintenanceAnchorInstallationV3(value) {
  requireExactKeys(value, ["anchorSha256", "contractVersion", "state"],
    "future-only maintenance anchor installation");
  requireSha256(value.anchorSha256,
    "future-only maintenance anchor installation anchorSha256");
  // admit only the one transferred installation state
  if (value.contractVersion !== "adjustment-maintenance-anchor-installation/v3" ||
    value.state !== "transferred") {
    throw new TypeError("future-only maintenance anchor installation is invalid");
  }
  return Object.freeze(value);
}

// validate one nullable exact current future-only maintenance anchor
export function validateAdjustmentMaintenanceAnchorCurrentV3(value) {
  // preserve exact genesis absence without manufacturing T or F authority
  if (value === null) {
    return null;
  }
  requireExactKeys(value, ["anchor", "anchorSha256"],
    "future-only maintenance anchor current");
  validateAdjustmentMaintenanceAnchorV3(value.anchor);
  requireSha256(value.anchorSha256,
    "future-only maintenance anchor current anchorSha256");
  // bind the returned identity to the complete canonical v3 anchor document
  if (sha256(canonicalJsonBytes(value.anchor)) !== value.anchorSha256) {
    throw new Error("future-only maintenance anchor current identity differs");
  }
  return Object.freeze(value);
}

// validate one typed future-only finalized-anchor response
export function validateAdjustmentMaintenanceAnchorFinalizationV3(value) {
  requireExactKeys(value, [
    "anchorSha256", "contractVersion", "finalizationProofSha256", "state",
  ], "future-only maintenance anchor finalization");
  requireSha256(value.anchorSha256,
    "future-only maintenance anchor finalization anchorSha256");
  requireSha256(value.finalizationProofSha256,
    "future-only maintenance anchor finalization proofSha256");
  // admit only the one finalized installation state
  if (value.contractVersion !== "adjustment-maintenance-anchor-finalization/v3" ||
    value.state !== "finalized") {
    throw new TypeError("future-only maintenance anchor finalization is invalid");
  }
  return Object.freeze(value);
}

// validate one value-free future-only family input closure
export function validateAdjustmentFutureOnlyInputSeal(value) {
  requireExactKeys(value, [
    "archiveCommitOrdinal", "burnSha256", "candidateArtifactRootSha256",
    "candidateReportSha256", "captureEpochWitnessSha256", "comparatorRootSha256",
    "confirmationAccessSha256", "confirmationChunkCount", "contractVersion",
    "custodyCheckpointSha256", "dueKey", "family", "frontierSha256",
    "fullMemberCount", "fullMemberRootSha256", "graphManifestSha256",
    "inputClasses", "lifecycleLedgerRootSha256", "pageSha256",
    "policyReportSha256", "predecessorSealSha256", "requiredInputRootSha256",
    "sealedAt", "sequence", "sourceCommit", "targetRootSha256",
    "workstationJournalHeadSha256",
  ], "future-only input seal");
  const dueMatch = /^confirmation\/(temperature|wind|rain)\/([a-f0-9]{64})$/u
    .exec(value.dueKey);

  // require the frozen family identity and terminal confirmation population
  if (value.contractVersion !== ADJUSTMENT_FUTURE_ONLY_INPUT_SEAL_CONTRACT_VERSION ||
    dueMatch === null || dueMatch[1] !== value.family ||
    value.fullMemberCount !== 1 ||
    value.confirmationChunkCount !== (value.family === "rain" ? 24 : 27) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("future-only input seal contract is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal, "future-only archiveCommitOrdinal");
  requireUint64Text(value.sequence, "future-only sequence");
  requireInstant(value.sealedAt, "future-only sealedAt");
  // validate every nonnullable cross-plane identity
  for (const field of [
    "burnSha256", "candidateArtifactRootSha256", "candidateReportSha256",
    "captureEpochWitnessSha256", "comparatorRootSha256", "confirmationAccessSha256",
    "custodyCheckpointSha256", "frontierSha256", "fullMemberRootSha256",
    "graphManifestSha256", "lifecycleLedgerRootSha256", "pageSha256",
    "policyReportSha256", "requiredInputRootSha256", "targetRootSha256",
    "workstationJournalHeadSha256",
  ]) {
    requireSha256(value[field], `future-only input seal ${field}`);
  }
  // permit only the genesis seal to omit its direct predecessor
  if (value.predecessorSealSha256 !== null) {
    requireSha256(value.predecessorSealSha256, "future-only predecessorSealSha256");
  }
  if ((value.sequence === "0") !== (value.predecessorSealSha256 === null)) {
    throw new TypeError("future-only input seal predecessor is invalid");
  }
  requireExactKeys(value.inputClasses, ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES,
    "future-only input classes");
  const emptyRootSha256 = sha256(canonicalJsonBytes([]));
  // validate every exact sorted-identity population root declaration
  for (const name of ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES) {
    const entry = value.inputClasses[name];
    requireExactKeys(entry, ["count", "rootSha256"], "future-only input class");
    requireSha256(entry.rootSha256, `future-only ${name} rootSha256`);
    // bound class cardinality and its unique empty representation
    if (!Number.isSafeInteger(entry.count) || entry.count < 0 || entry.count > 9_500_000 ||
      (entry.count === 0 && entry.rootSha256 !== emptyRootSha256)) {
      throw new TypeError("future-only input class population is invalid");
    }
  }
  const requiredNonempty = value.family === "rain"
    ? ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES
    : ADJUSTMENT_FUTURE_INPUT_CLASS_NAMES.filter((name) => name !== "rain_gate_input");
  // require only genuine family inputs and prohibit synthetic rain inputs elsewhere
  if (requiredNonempty.some((name) => value.inputClasses[name].count < 1) ||
    (value.family !== "rain" &&
      (value.inputClasses.rain_gate_input.count !== 0 ||
        value.inputClasses.rain_gate_input.rootSha256 !== emptyRootSha256))) {
    throw new TypeError("future-only family input population is incomplete");
  }
  // bind the named roots to the closed input-class document
  if (value.requiredInputRootSha256 !== sha256(canonicalJsonBytes(value.inputClasses)) ||
    value.comparatorRootSha256 !== value.inputClasses.comparator.rootSha256 ||
    value.targetRootSha256 !== value.inputClasses.target.rootSha256 ||
    value.inputClasses.candidate.count !== 1 ||
    value.inputClasses.candidate.rootSha256 !== sha256(canonicalJsonBytes([dueMatch[2]])) ||
    value.candidateArtifactRootSha256 !== sha256(canonicalJsonBytes([
      value.inputClasses.candidate.rootSha256,
      value.inputClasses.artifact.rootSha256,
    ]))) {
    throw new Error("future-only input seal roots differ");
  }
  return Object.freeze(value);
}

// compare every future-only seal identity copied into one transferred anchor
function futureAnchorMatchesSeal(anchor, seal) {
  const bindings = [
    "archiveCommitOrdinal", "burnSha256", "candidateArtifactRootSha256",
    "candidateReportSha256", "captureEpochWitnessSha256",
    "confirmationAccessSha256", "confirmationChunkCount",
    "custodyCheckpointSha256", "dueKey", "family", "frontierSha256",
    "fullMemberRootSha256", "graphManifestSha256", "lifecycleLedgerRootSha256",
    "pageSha256", "policyReportSha256", "requiredInputRootSha256", "sequence",
    "sourceCommit", "workstationJournalHeadSha256",
  ];
  return bindings.every(
    // require byte-identical authority across the seal-to-anchor transition
    (field) => anchor[field] === seal[field],
  );
}

// compare every transferred anchor identity copied into one finalization proof
function futureFinalizationMatchesAnchor(proof, anchor) {
  const bindings = [
    "actionSha256", "archiveCommitOrdinal", "burnSha256",
    "candidateArtifactRootSha256", "candidateReportSha256",
    "captureEpochWitnessSha256", "confirmationAccessSha256",
    "confirmationChunkCount", "controlSha256", "controlVersion",
    "custodyCheckpointSha256", "dueKey", "family", "frontierSha256",
    "fullGraphVerifiedAt", "fullMemberRootSha256", "graphManifestSha256",
    "inputSealSha256", "lifecycleLedgerRootSha256", "pageSha256",
    "policyReportSha256", "predecessorAnchorSha256", "publishedAt",
    "requiredInputRootSha256", "sequence", "sourceCommit",
    "workstationJournalHeadSha256",
  ];
  return bindings.every(
    // require F to preserve every transferred authority identity
    (field) => proof[field] === anchor[field],
  );
}

// recognize only the exact finalized successor of one supplied transferred anchor
function futureFinalizedAnchorSucceedsTransfer(finalized, transferred, transferredSha256) {
  // keep this replay path disjoint from legacy anchors and ordinary successor installs
  if (finalized.contractVersion !== ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION ||
    finalized.ctfState !== "finalized" || transferred.ctfState !== "transferred") {
    return false;
  }
  const { contractVersion: _contractVersion, ctfState: _ctfState, ...proofFields } =
    transferred;
  const proof = validateAdjustmentMaintenanceFinalizationProofV3({
    ...proofFields,
    contractVersion: ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_V3_CONTRACT_VERSION,
    finalizedAt: finalized.finalizedAt,
    transferredAnchorSha256: transferredSha256,
  });
  const proofSha256 = sha256(canonicalJsonBytes(proof));
  const expectedFinalized = validateAdjustmentMaintenanceAnchorV3({
    ...transferred,
    ctfState: "finalized",
    finalizationProofSha256: proofSha256,
    finalizedAt: finalized.finalizedAt,
  });
  return finalized.finalizationProofSha256 === proofSha256 &&
    canonicalJson(finalized) === canonicalJson(expectedFinalized);
}

// validate one transferred or finalized future-only maintenance anchor
export function validateAdjustmentMaintenanceAnchorV3(value) {
  const baseKeys = [
    "actionSha256", "archiveCommitOrdinal", "burnSha256",
    "candidateArtifactRootSha256", "candidateReportSha256",
    "captureEpochWitnessSha256", "confirmationAccessSha256",
    "confirmationChunkCount", "contractVersion", "controlSha256", "controlVersion",
    "ctfState", "custodyCheckpointSha256", "dueKey", "family", "frontierSha256",
    "fullGraphVerifiedAt", "fullMemberRootSha256", "graphManifestSha256",
    "inputSealSha256", "lifecycleLedgerRootSha256", "pageSha256",
    "policyReportSha256", "predecessorAnchorSha256", "publishedAt",
    "requiredInputRootSha256", "sequence", "sourceCommit",
    "workstationJournalHeadSha256",
  ];
  const finalizedKeys = ["finalizationProofSha256", "finalizedAt"];
  requireExactKeys(value, value?.ctfState === "finalized"
    ? [...baseKeys, ...finalizedKeys] : baseKeys, "future-only maintenance anchor");
  const dueMatch = /^confirmation\/(temperature|wind|rain)\/[a-f0-9]{64}$/u
    .exec(value.dueKey);

  // require one frozen versioned state without nullable action authority
  if (value.contractVersion !== ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION ||
    !["transferred", "finalized"].includes(value.ctfState) || dueMatch === null ||
    dueMatch[1] !== value.family ||
    value.confirmationChunkCount !== (value.family === "rain" ? 24 : 27) ||
    !/^[1-9][0-9]{0,5}$/u.test(value.controlVersion) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("future-only maintenance anchor contract is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal, "future-only anchor archiveCommitOrdinal");
  requireUint64Text(value.sequence, "future-only anchor sequence");
  requireInstant(value.fullGraphVerifiedAt, "future-only anchor fullGraphVerifiedAt");
  requireInstant(value.publishedAt, "future-only anchor publishedAt");
  // validate every nonnullable future-only authority identity
  for (const field of [
    "actionSha256", "burnSha256", "candidateArtifactRootSha256",
    "candidateReportSha256", "captureEpochWitnessSha256", "confirmationAccessSha256",
    "controlSha256", "custodyCheckpointSha256", "frontierSha256",
    "fullMemberRootSha256", "graphManifestSha256", "inputSealSha256",
    "lifecycleLedgerRootSha256", "pageSha256", "policyReportSha256",
    "requiredInputRootSha256", "workstationJournalHeadSha256",
  ]) {
    requireSha256(value[field], `future-only maintenance anchor ${field}`);
  }
  // permit only the first anchor to omit its direct predecessor
  if (value.predecessorAnchorSha256 !== null) {
    requireSha256(value.predecessorAnchorSha256,
      "future-only maintenance anchor predecessorAnchorSha256");
  }
  if ((value.sequence === "0") !== (value.predecessorAnchorSha256 === null) ||
    Date.parse(value.fullGraphVerifiedAt) > Date.parse(value.publishedAt)) {
    throw new TypeError("future-only maintenance anchor predecessor is invalid");
  }
  // validate only the additive finalization identities in F state
  if (value.ctfState === "finalized") {
    requireSha256(value.finalizationProofSha256,
      "future-only maintenance anchor finalizationProofSha256");
    requireInstant(value.finalizedAt, "future-only maintenance anchor finalizedAt");
    if (Date.parse(value.finalizedAt) < Date.parse(value.publishedAt)) {
      throw new TypeError("future-only maintenance anchor finalization clock is invalid");
    }
  }
  return Object.freeze(value);
}

// validate one external future-only finalization proof without self-hashing
export function validateAdjustmentMaintenanceFinalizationProofV3(value) {
  requireExactKeys(value, [
    "actionSha256", "archiveCommitOrdinal", "burnSha256",
    "candidateArtifactRootSha256", "candidateReportSha256",
    "captureEpochWitnessSha256", "confirmationAccessSha256",
    "confirmationChunkCount", "contractVersion", "controlSha256", "controlVersion",
    "custodyCheckpointSha256", "dueKey", "family", "finalizedAt", "frontierSha256",
    "fullGraphVerifiedAt", "fullMemberRootSha256", "graphManifestSha256",
    "inputSealSha256", "lifecycleLedgerRootSha256", "pageSha256",
    "policyReportSha256", "predecessorAnchorSha256", "publishedAt",
    "requiredInputRootSha256", "sequence", "sourceCommit",
    "transferredAnchorSha256", "workstationJournalHeadSha256",
  ], "future-only maintenance finalization proof");
  const { contractVersion, finalizedAt, transferredAnchorSha256, ...anchorFields } = value;
  const anchor = validateAdjustmentMaintenanceAnchorV3({
    ...anchorFields,
    contractVersion: ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION,
    ctfState: "transferred",
  });
  requireInstant(finalizedAt, "future-only finalization finalizedAt");
  requireSha256(transferredAnchorSha256,
    "future-only finalization transferredAnchorSha256");

  // require the proof clock to follow the graph publication clock
  if (contractVersion !== ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_V3_CONTRACT_VERSION ||
    Date.parse(finalizedAt) < Date.parse(anchor.publishedAt)) {
    throw new TypeError("future-only maintenance finalization proof is invalid");
  }
  return Object.freeze(value);
}

// validate one custody-only development action anchor without C/T/F authority
export function validateAdjustmentDevelopmentCustodyAnchorV1(value) {
  requireExactKeys(value, [
    "actionKind", "actionSha256", "archiveCommitOrdinal", "artifactSha256",
    "candidateGraphSha256", "candidateSha256", "captureEpochWitnessSha256",
    "contractVersion", "controlSha256", "controlVersion", "custodyAcknowledgedAt",
    "custodyAcknowledgementSha256", "custodyCheckpointSha256",
    "developmentGraphSha256", "dueKey", "family", "frontierSha256",
    "fullGraphVerifiedAt", "inputHeadSha256", "lifecycleLedgerRootSha256",
    "memberRootSha256", "pageSha256", "policyReportSha256",
    "predecessorAnchorSha256", "registrationSha256", "sequence", "sourceCommit",
    "sourceSha256", "startMemberSha256", "startSha256",
  ], "adjustment development custody anchor");
  const due = /^monthly\/(temperature|wind|rain)\/(\d{4}-\d{2})$/u.exec(value.dueKey);
  // keep development custody separate from terminal model authority
  if (value.contractVersion !== ADJUSTMENT_DEVELOPMENT_CUSTODY_ANCHOR_CONTRACT_VERSION ||
    value.actionKind !== "shadow" || due === null || due[1] !== value.family ||
    !/^[1-9][0-9]{0,5}$/u.test(value.controlVersion) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("adjustment development custody anchor contract is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal,
    "adjustment development custody archive ordinal");
  requireUint64Text(value.sequence, "adjustment development custody sequence");
  requireInstant(value.custodyAcknowledgedAt,
    "adjustment development custody acknowledged time");
  requireInstant(value.fullGraphVerifiedAt,
    "adjustment development custody graph time");
  // bind every public action, graph and custody identity
  for (const field of [
    "actionSha256", "artifactSha256", "candidateGraphSha256", "candidateSha256",
    "captureEpochWitnessSha256", "controlSha256", "custodyAcknowledgementSha256",
    "custodyCheckpointSha256", "developmentGraphSha256", "frontierSha256",
    "inputHeadSha256", "lifecycleLedgerRootSha256", "memberRootSha256",
    "pageSha256", "policyReportSha256", "registrationSha256", "sourceSha256",
    "startSha256",
  ]) {
    requireSha256(value[field], `adjustment development custody ${field}`);
  }
  // permit nullable identities only at their genuine first boundaries
  for (const field of ["predecessorAnchorSha256", "startMemberSha256"]) {
    if (value[field] !== null) {
      requireSha256(value[field], `adjustment development custody ${field}`);
    }
  }
  if ((value.sequence === "0") !== (value.predecessorAnchorSha256 === null) ||
    Date.parse(value.fullGraphVerifiedAt) < Date.parse(value.custodyAcknowledgedAt)) {
    throw new TypeError("adjustment development custody predecessor is invalid");
  }
  return Object.freeze(value);
}

// validate one typed development custody installation response
export function validateAdjustmentDevelopmentCustodyAnchorInstallation(value) {
  requireExactKeys(value, ["anchorSha256", "contractVersion", "state"],
    "adjustment development custody installation");
  requireSha256(value.anchorSha256,
    "adjustment development custody installation anchor");
  if (value.contractVersion !==
      "adjustment-development-custody-anchor-installation/v1" ||
    value.state !== "installed") {
    throw new TypeError("adjustment development custody installation is invalid");
  }
  return Object.freeze(value);
}

// validate one nullable current development custody projection
export function validateAdjustmentDevelopmentCustodyAnchorCurrent(value) {
  // preserve the empty create-once state without inventing a sequence
  if (value === null) {
    return null;
  }
  requireExactKeys(value, ["anchor", "anchorSha256"],
    "adjustment development custody current");
  validateAdjustmentDevelopmentCustodyAnchorV1(value.anchor);
  requireSha256(value.anchorSha256,
    "adjustment development custody current anchor");
  // bind the returned identity to the exact canonical anchor bytes
  if (sha256(canonicalJsonBytes(value.anchor)) !== value.anchorSha256) {
    throw new Error("adjustment development custody current identity differs");
  }
  return Object.freeze(value);
}

// validate one custody-only rain control-reference action anchor
export function validateAdjustmentRainControlCustodyAnchorV1(value) {
  requireExactKeys(value, [
    "actionKind", "actionSha256", "archiveCommitOrdinal",
    "captureEpochWitnessSha256", "contractVersion", "controlSha256",
    "controlStateSha256", "controlVersion", "custodyAcknowledgedAt",
    "custodyAcknowledgementSha256", "custodyCheckpointSha256", "dueMonth",
    "fencingToken", "frontierSha256", "fullGraphVerifiedAt",
    "graphManifestSha256", "memberRootSha256", "ordinalArtifactSha256",
    "pageSha256", "predecessorAnchorSha256", "sequence", "sourceCommit",
    "sourceMemberRootSha256", "sourceReceiptRootSha256",
    "startMemberSha256", "startSha256", "workstationJournalHeadSha256",
  ], "adjustment rain control custody anchor");
  // keep control-reference custody disjoint from model and C/T/F authority
  if (value.contractVersion !== ADJUSTMENT_RAIN_CONTROL_CUSTODY_ANCHOR_CONTRACT_VERSION ||
    value.actionKind !== "control_reference" ||
    !/^20\d{2}-(?:0[1-9]|1[0-2])$/u.test(value.dueMonth) ||
    !/^[1-9][0-9]{0,5}$/u.test(value.controlVersion) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("adjustment rain control custody anchor contract is invalid");
  }
  for (const field of ["archiveCommitOrdinal", "fencingToken", "sequence"]) {
    requireUint64Text(value[field], `adjustment rain control custody ${field}`);
  }
  requireInstant(value.custodyAcknowledgedAt,
    "adjustment rain control custody acknowledged time");
  requireInstant(value.fullGraphVerifiedAt,
    "adjustment rain control custody graph time");
  // bind every public action, state, source graph and packed-custody identity
  for (const field of [
    "actionSha256", "captureEpochWitnessSha256", "controlSha256",
    "controlStateSha256", "custodyAcknowledgementSha256",
    "custodyCheckpointSha256", "frontierSha256", "graphManifestSha256",
    "memberRootSha256", "ordinalArtifactSha256", "pageSha256",
    "sourceMemberRootSha256", "sourceReceiptRootSha256", "startSha256",
    "workstationJournalHeadSha256",
  ]) {
    requireSha256(value[field], `adjustment rain control custody ${field}`);
  }
  for (const field of ["predecessorAnchorSha256", "startMemberSha256"]) {
    // permit only the two protocol-defined nullable boundaries
    if (value[field] !== null) {
      requireSha256(value[field], `adjustment rain control custody ${field}`);
    }
  }
  if ((value.sequence === "0") !== (value.predecessorAnchorSha256 === null) ||
    Date.parse(value.fullGraphVerifiedAt) < Date.parse(value.custodyAcknowledgedAt)) {
    throw new TypeError("adjustment rain control custody predecessor is invalid");
  }
  return Object.freeze(value);
}

// validate one typed rain control custody installation response
export function validateAdjustmentRainControlCustodyAnchorInstallation(value) {
  requireExactKeys(value, ["anchorSha256", "contractVersion", "state"],
    "adjustment rain control custody installation");
  requireSha256(value.anchorSha256,
    "adjustment rain control custody installation anchor");
  if (value.contractVersion !==
      "adjustment-rain-control-custody-anchor-installation/v1" ||
    value.state !== "installed") {
    throw new TypeError("adjustment rain control custody installation is invalid");
  }
  return Object.freeze(value);
}

// validate one nullable current rain control custody projection
export function validateAdjustmentRainControlCustodyAnchorCurrent(value) {
  // preserve the empty create-once state without inventing authority
  if (value === null) {
    return null;
  }
  requireExactKeys(value, ["anchor", "anchorSha256"],
    "adjustment rain control custody current");
  validateAdjustmentRainControlCustodyAnchorV1(value.anchor);
  requireSha256(value.anchorSha256,
    "adjustment rain control custody current anchor");
  // bind the response identity to the exact canonical anchor
  if (sha256(canonicalJsonBytes(value.anchor)) !== value.anchorSha256) {
    throw new Error("adjustment rain control custody current identity differs");
  }
  return Object.freeze(value);
}

// parse one canonical bounded rain control custody anchor
function parseAdjustmentRainControlCustodyAnchor(bytesValue) {
  if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
    bytesValue.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new TypeError("adjustment rain control custody anchor bytes are invalid");
  }
  const value = JSON.parse(bytesValue.toString("utf8"));
  validateAdjustmentRainControlCustodyAnchorV1(value);
  // prohibit alternate encodings at the privileged boundary
  if (!bytesValue.equals(canonicalJsonBytes(value))) {
    throw new TypeError("adjustment rain control custody anchor is not canonical");
  }
  return value;
}

// parse one canonical bounded unsupported terminal proof
function parseAdjustmentUnsupportedTerminalProof(bytesValue) {
  if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
    bytesValue.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new TypeError("adjustment unsupported terminal proof bytes are invalid");
  }
  const value = JSON.parse(bytesValue.toString("utf8"));
  validateAdjustmentUnsupportedTerminalProofV1(value);
  // prohibit alternate encodings at the privileged boundary
  if (!bytesValue.equals(canonicalJsonBytes(value))) {
    throw new TypeError("adjustment unsupported terminal proof is not canonical");
  }
  return value;
}

// parse one canonical bounded development custody anchor
function parseAdjustmentDevelopmentCustodyAnchor(bytesValue) {
  if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
    bytesValue.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new TypeError("adjustment development custody anchor bytes are invalid");
  }
  const value = JSON.parse(bytesValue.toString("utf8"));
  validateAdjustmentDevelopmentCustodyAnchorV1(value);
  // prohibit alternate encodings at the privileged boundary
  if (!bytesValue.equals(canonicalJsonBytes(value))) {
    throw new TypeError("adjustment development custody anchor is not canonical");
  }
  return value;
}

// parse one canonical bounded future-only input seal
function parseAdjustmentFutureOnlyInputSeal(bytesValue) {
  // require one complete canonical seal document
  if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
    bytesValue.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new TypeError("future-only input seal bytes are invalid");
  }
  let value;
  try {
    value = JSON.parse(bytesValue.toString("utf8"));
  } catch {
    throw new TypeError("future-only input seal JSON is invalid");
  }
  validateAdjustmentFutureOnlyInputSeal(value);
  // reject alternate encodings of the same validated seal
  if (!bytesValue.equals(canonicalJsonBytes(value))) {
    throw new TypeError("future-only input seal is not canonical");
  }
  return value;
}

// retain one predecessor-linked future-only seal after exact custody
export class AdjustmentFutureOnlyInputSealStore {
  #captureEpochPath;
  #expectedControlSha256;
  #expectedControlVersion;
  #readCaptureEpochWitness;
  #readSourceClosure;
  #root;
  #sealRoot;

  // retain only the fixed evidence and root-owned witness paths
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#sealRoot = join(this.#root, "future-input-seals");
    this.#captureEpochPath = options.captureEpochPath ??
      ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH;
    this.#expectedControlSha256 = options.expectedControlSha256 ??
      process.env.WEATHER_ANCHOR_CONTROL_SHA256 ?? null;
    this.#expectedControlVersion = options.expectedControlVersion ??
      process.env.WEATHER_ANCHOR_CONTROL_VERSION ?? null;
    this.#readCaptureEpochWitness = options.readCaptureEpochWitness ??
      (() => readAdjustmentRevisionCaptureEpochWitness({
        expectedGid: 0,
        expectedUid: 0,
        path: this.#captureEpochPath,
      }));
    this.#readSourceClosure = options.readSourceClosure ?? (async (sourceCommit) => {
      const { verifyAdjustmentFamilyReleaseAncestor } =
        await import("./adjustment-evaluation-package.mjs");
      return await verifyAdjustmentFamilyReleaseAncestor(sourceCommit);
    });
  }

  // install one canonical seal after exact epoch and custody crossbinding
  async install(bytesValue, expectedSha256) {
    requireSha256(expectedSha256, "future-only input seal SHA256");
    // reject implicit coercion at the privileged installation boundary
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("future-only input seal bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const seal = parseAdjustmentFutureOnlyInputSeal(bytes);
    // bind the caller operand to the exact canonical seal
    if (sha256(bytes) !== expectedSha256) {
      throw new Error("future-only input seal identity is invalid");
    }
    await requireRevisionCustodyDirectories(this.#root);
    const custodyState = await readRevisionCustodyAcknowledgements(this.#root);
    const custody = [custodyState.current, custodyState.previous].find(
      // accept only the exact retained packed-custody checkpoint
      (entry) => entry !== null &&
        entry.contractVersion === ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION &&
        entry.custodyCheckpointSha256 === seal.custodyCheckpointSha256 &&
        entry.pageSha256 === seal.pageSha256,
    );
    // crossbind the terminal cursor without treating custody as qualification
    if (custody === undefined ||
      custody.nextArchiveCommitOrdinal !== seal.archiveCommitOrdinal ||
      custody.nextFrontierSha256 !== seal.frontierSha256) {
      throw new Error("future-only input seal custody differs");
    }
    const witness = validateAdjustmentRevisionCaptureEpochWitness(
      await this.#readCaptureEpochWitness(),
    );
    // bind the actual retained future-only epoch without exposing its fields
    if (sha256(canonicalJsonBytes(witness)) !== seal.captureEpochWitnessSha256) {
      throw new Error("future-only input seal epoch differs");
    }
    await this.#ensureRoot();
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    await this.#readSlot("previous");

    // converge only the exact first installed seal bytes
    if (current !== null && current.bytes.equals(bytes)) {
      if (pending !== null && !pending.bytes.equals(bytes)) {
        throw new Error("future-only input seal pending collision");
      }
      if (pending !== null) {
        await unlink(join(this.#sealRoot, "pending.json"));
        await fsyncDirectory(this.#sealRoot);
      }
      return validateAdjustmentFutureOnlyInputSealInstallation({
        contractVersion: "adjustment-future-only-input-seal-installation/v2",
        sealSha256: expectedSha256,
        state: "installed",
      });
    }
    const currentSha256 = current === null ? null : sha256(current.bytes);
    const expectedSequence = current === null ? 0n : BigInt(current.seal.sequence) + 1n;
    // require one direct immutable seal predecessor without reset
    if (seal.predecessorSealSha256 !== currentSha256 ||
      BigInt(seal.sequence) !== expectedSequence) {
      throw new Error("future-only input seal predecessor is invalid");
    }
    // retain only one byte-identical pending transaction
    if (pending !== null) {
      if (!pending.bytes.equals(bytes)) {
        throw new Error("future-only input seal pending collision");
      }
    } else {
      await writeExclusive(join(this.#sealRoot, "pending.json"), bytes);
    }
    await this.#commitPending(expectedSha256);
    return validateAdjustmentFutureOnlyInputSealInstallation({
      contractVersion: "adjustment-future-only-input-seal-installation/v2",
      sealSha256: expectedSha256,
      state: "installed",
    });
  }

  // read only the current exact seal for anchor crossbinding
  async readCurrent() {
    await this.#requireRoot(await lstat(this.#sealRoot, { bigint: true }));
    const current = await this.#readSlot("current");
    return current === null ? null : Object.freeze({
      seal: current.seal,
      sealSha256: sha256(current.bytes),
    });
  }

  // read one current seal only after active epoch, control and source closure
  async readCurrentVerified() {
    const witness = validateAdjustmentRevisionCaptureEpochWitness(
      await this.#readCaptureEpochWitness(),
    );
    // require the actual active v14 control before exposing null or proof bytes
    if (this.#expectedControlVersion !== "14" ||
      this.#expectedControlVersion !== witness.controlPlaneVersion ||
      this.#expectedControlSha256 !== witness.controlPlaneSha256) {
      throw new Error("future-only input seal active epoch differs");
    }
    let details;
    try {
      details = await lstat(this.#sealRoot, { bigint: true });
    } catch (error) {
      // expose only genuine genesis absence as canonical null
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    await this.#requireRoot(details);
    const names = await readdir(this.#sealRoot);
    // reject foreign state outside the fixed bounded seal transaction slots
    if (names.some((name) =>
      !["current.json", "pending.json", "previous.json"].includes(name))) {
      throw new Error("future-only input seal root contains an unknown entry");
    }
    const current = await this.#readSlot("current");
    // preserve an initialized empty root as the same null genesis marker
    if (current === null) {
      return null;
    }
    // require the complete retained epoch binding before exposing proof bytes
    if (current.seal.captureEpochWitnessSha256 !== sha256(canonicalJsonBytes(witness))) {
      throw new Error("future-only input seal active epoch differs");
    }
    // accept only a source proven on the retained root-owned release ancestry
    if (await this.#readSourceClosure(current.seal.sourceCommit) !== true) {
      throw new Error("future-only input seal source closure differs");
    }
    return validateAdjustmentFutureOnlyInputSealCurrent({
      seal: current.seal,
      sealSha256: sha256(current.bytes),
    });
  }

  // create only the fixed root-owned compact seal directory
  async #ensureRoot() {
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(this.#sealRoot);
    await this.#requireRoot(await lstat(this.#sealRoot, { bigint: true }));
  }

  // read one bounded root-owner seal slot without following links
  async #readSlot(slot) {
    const path = join(this.#sealRoot, `${slot}.json`);
    let handle;
    try {
      handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    } catch (error) {
      // distinguish only genuine absence from unsafe slot state
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    try {
      const before = await handle.stat({ bigint: true });
      // require one bounded single-link file owned by the installing identity
      if (!before.isFile() || before.uid !== BigInt(process.getuid()) ||
        before.gid !== BigInt(process.getgid()) || before.nlink !== 1n ||
        (before.mode & 0o777n) !== 0o600n || before.size < 1n ||
        before.size > BigInt(ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES)) {
        throw new Error("future-only input seal slot is unsafe");
      }
      const bytes = await handle.readFile();
      const after = await handle.stat({ bigint: true });
      // reject replacement or mutation during the bounded read
      if (before.dev !== after.dev || before.ino !== after.ino ||
        before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
        before.ctimeNs !== after.ctimeNs) {
        throw new Error("future-only input seal slot changed while reading");
      }
      return { bytes, seal: parseAdjustmentFutureOnlyInputSeal(bytes) };
    } finally {
      await handle.close();
    }
  }

  // finish one pending seal transaction with current and previous slots
  async #commitPending(expectedSha256) {
    const pending = await this.#readSlot("pending");
    const current = await this.#readSlot("current");
    // accept only the exact pending payload or an already committed retry
    if (pending === null) {
      if (current !== null && sha256(current.bytes) === expectedSha256) {
        return;
      }
      throw new Error("future-only input seal pending state is unavailable");
    }
    if (sha256(pending.bytes) !== expectedSha256) {
      throw new Error("future-only input seal pending identity is invalid");
    }
    // retain the direct predecessor before replacing current
    if (current !== null && sha256(current.bytes) !== expectedSha256) {
      await writePrivateAtomic(join(this.#sealRoot, "previous.json"), current.bytes);
    }
    await writePrivateAtomic(join(this.#sealRoot, "current.json"), pending.bytes);
    await unlink(join(this.#sealRoot, "pending.json"));
    await fsyncDirectory(this.#sealRoot);
  }

  // require exact root ownership for the compact seal control boundary
  async #requireRoot(details) {
    // reject links, aliases, foreign owners and broad modes
    if (!details.isDirectory() || details.isSymbolicLink() ||
      details.uid !== BigInt(process.getuid()) || details.gid !== BigInt(process.getgid()) ||
      (details.mode & 0o777n) !== 0o700n ||
      await realpath(this.#sealRoot) !== this.#sealRoot) {
      throw new Error("future-only input seal root is unsafe");
    }
  }
}

// retain shadow-only development custody without creating C/T/F authority
export class AdjustmentDevelopmentCustodyAnchorStore {
  #anchorRoot;
  #captureEpochPath;
  #expectedControlSha256;
  #expectedControlVersion;
  #readCaptureEpochWitness;
  #root;

  // retain only fixed evidence, witness and control identities
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#anchorRoot = join(this.#root, "development-custody-anchors");
    this.#captureEpochPath = options.captureEpochPath ??
      ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH;
    this.#expectedControlSha256 = options.expectedControlSha256 ??
      process.env.WEATHER_ANCHOR_CONTROL_SHA256 ?? null;
    this.#expectedControlVersion = options.expectedControlVersion ??
      process.env.WEATHER_ANCHOR_CONTROL_VERSION ?? null;
    this.#readCaptureEpochWitness = options.readCaptureEpochWitness ??
      (() => readAdjustmentRevisionCaptureEpochWitness({
        expectedGid: 0,
        expectedUid: 0,
        path: this.#captureEpochPath,
      }));
  }

  // install one action anchor after actual packed custody and epoch proof
  async install(bytesValue, expectedSha256) {
    requireSha256(expectedSha256, "adjustment development custody anchor SHA256");
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("adjustment development custody anchor bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const anchor = parseAdjustmentDevelopmentCustodyAnchor(bytes);
    // bind the operand and active control identity before state mutation
    if (sha256(bytes) !== expectedSha256 ||
      this.#expectedControlSha256 === null || this.#expectedControlVersion === null ||
      anchor.controlSha256 !== this.#expectedControlSha256 ||
      anchor.controlVersion !== this.#expectedControlVersion) {
      throw new Error("adjustment development custody anchor identity is invalid");
    }
    await requireRevisionCustodyDirectories(this.#root);
    const custodyState = await readRevisionCustodyAcknowledgements(this.#root);
    const custody = [custodyState.current, custodyState.previous].find(
      // select only the exact retained packed-custody acknowledgement
      (entry) => entry !== null &&
        entry.contractVersion === ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION &&
        entry.acknowledgementSha256 === anchor.custodyAcknowledgementSha256 &&
        entry.custodyCheckpointSha256 === anchor.custodyCheckpointSha256 &&
        entry.pageSha256 === anchor.pageSha256,
    );
    // crossbind every copied custody coordinate without granting semantic authority
    if (custody === undefined ||
      custody.acknowledgedAt !== anchor.custodyAcknowledgedAt ||
      custody.nextArchiveCommitOrdinal !== anchor.archiveCommitOrdinal ||
      custody.nextFrontierSha256 !== anchor.frontierSha256 ||
      custody.memberRootSha256 !== anchor.memberRootSha256 ||
      custody.startMemberSha256 !== anchor.startMemberSha256 ||
      custody.startSha256 !== anchor.startSha256) {
      throw new Error("adjustment development custody acknowledgement differs");
    }
    const witness = validateAdjustmentRevisionCaptureEpochWitness(
      await this.#readCaptureEpochWitness(),
    );
    // bind development bytes to the retained future-only epoch
    if (sha256(canonicalJsonBytes(witness)) !== anchor.captureEpochWitnessSha256) {
      throw new Error("adjustment development custody epoch differs");
    }
    await this.#ensureRoot();
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    await this.#readSlot("previous");
    // converge only byte-identical installed retries
    if (current !== null && current.bytes.equals(bytes)) {
      if (pending !== null && !pending.bytes.equals(bytes)) {
        throw new Error("adjustment development custody pending collision");
      }
      if (pending !== null) {
        await unlink(join(this.#anchorRoot, "pending.json"));
        await fsyncDirectory(this.#anchorRoot);
      }
      return validateAdjustmentDevelopmentCustodyAnchorInstallation({
        anchorSha256: expectedSha256,
        contractVersion: "adjustment-development-custody-anchor-installation/v1",
        state: "installed",
      });
    }
    const currentSha256 = current === null ? null : sha256(current.bytes);
    const expectedSequence = current === null ? 0n : BigInt(current.anchor.sequence) + 1n;
    // require an exact predecessor and monotonic sequence
    if (anchor.predecessorAnchorSha256 !== currentSha256 ||
      BigInt(anchor.sequence) !== expectedSequence) {
      throw new Error("adjustment development custody predecessor is invalid");
    }
    // preserve only one byte-identical in-progress transaction
    if (pending !== null) {
      if (!pending.bytes.equals(bytes)) {
        throw new Error("adjustment development custody pending collision");
      }
    } else {
      await writeExclusive(join(this.#anchorRoot, "pending.json"), bytes);
    }
    await this.#commitPending(expectedSha256);
    return validateAdjustmentDevelopmentCustodyAnchorInstallation({
      anchorSha256: expectedSha256,
      contractVersion: "adjustment-development-custody-anchor-installation/v1",
      state: "installed",
    });
  }

  // return only the exact current custody anchor and its identity
  async readCurrent() {
    let details;
    try {
      details = await lstat(this.#anchorRoot, { bigint: true });
    } catch (error) {
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    await this.#requireRoot(details);
    const current = await this.#readSlot("current");
    return validateAdjustmentDevelopmentCustodyAnchorCurrent(current === null ? null : {
      anchor: current.anchor,
      anchorSha256: sha256(current.bytes),
    });
  }

  // authorize only one exact inactive shadow action
  async authorizeDevelopmentShadowAction(input) {
    requireExactKeys(input, [
      "actionKind", "actionSha256", "artifactSha256", "candidateGraphSha256",
      "candidateSha256", "family", "lifecycleLedgerRootSha256",
      "policyReportSha256", "registrationSha256", "sourceCommit",
    ], "adjustment development shadow authorization");
    if (input.actionKind !== "shadow" ||
      !["rain", "temperature", "wind"].includes(input.family) ||
      !/^[a-f0-9]{40}$/u.test(input.sourceCommit)) {
      throw new TypeError("adjustment development shadow authorization is invalid");
    }
    // validate every action-carried content identity
    for (const field of [
      "actionSha256", "artifactSha256", "candidateGraphSha256", "candidateSha256",
      "lifecycleLedgerRootSha256", "policyReportSha256", "registrationSha256",
    ]) {
      requireSha256(input[field], `adjustment development shadow ${field}`);
    }
    const current = await this.readCurrent();
    // require byte-identical action authority without accepting compensation or F
    if (current === null || Object.entries(input).some(
      ([key, value]) => current.anchor[key] !== value,
    )) {
      throw new Error("adjustment development custody authorization differs");
    }
    return Object.freeze({ ...current.anchor });
  }

  // create only the fixed private development anchor root
  async #ensureRoot() {
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(this.#anchorRoot);
    await this.#requireRoot(await lstat(this.#anchorRoot, { bigint: true }));
  }

  // read one bounded development anchor slot without following links
  async #readSlot(slot) {
    const path = join(this.#anchorRoot, `${slot}.json`);
    let handle;
    try {
      handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    } catch (error) {
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    try {
      const before = await handle.stat({ bigint: true });
      // require one bounded single-link owner-only slot
      if (!before.isFile() || before.uid !== BigInt(process.getuid()) ||
        before.gid !== BigInt(process.getgid()) || before.nlink !== 1n ||
        (before.mode & 0o777n) !== 0o600n || before.size < 1n ||
        before.size > BigInt(ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES)) {
        throw new Error("adjustment development custody slot is unsafe");
      }
      const bytes = await handle.readFile();
      const after = await handle.stat({ bigint: true });
      // reject replacement or mutation during the bounded read
      if (before.dev !== after.dev || before.ino !== after.ino ||
        before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
        before.ctimeNs !== after.ctimeNs) {
        throw new Error("adjustment development custody slot changed while reading");
      }
      return { anchor: parseAdjustmentDevelopmentCustodyAnchor(bytes), bytes };
    } finally {
      await handle.close();
    }
  }

  // commit one pending anchor while retaining only its direct predecessor
  async #commitPending(expectedSha256) {
    const pending = await this.#readSlot("pending");
    const current = await this.#readSlot("current");
    if (pending === null) {
      if (current !== null && sha256(current.bytes) === expectedSha256) {
        return;
      }
      throw new Error("adjustment development custody pending state is unavailable");
    }
    if (sha256(pending.bytes) !== expectedSha256) {
      throw new Error("adjustment development custody pending identity is invalid");
    }
    if (current !== null && sha256(current.bytes) !== expectedSha256) {
      await writePrivateAtomic(join(this.#anchorRoot, "previous.json"), current.bytes);
    }
    await writePrivateAtomic(join(this.#anchorRoot, "current.json"), pending.bytes);
    await unlink(join(this.#anchorRoot, "pending.json"));
    await fsyncDirectory(this.#anchorRoot);
  }

  // require one exact owner-private anchor directory
  async #requireRoot(details) {
    if (!details.isDirectory() || details.isSymbolicLink() ||
      details.uid !== BigInt(process.getuid()) || details.gid !== BigInt(process.getgid()) ||
      (details.mode & 0o777n) !== 0o700n ||
      await realpath(this.#anchorRoot) !== this.#anchorRoot) {
      throw new Error("adjustment development custody root is unsafe");
    }
  }
}

// retain rain control-reference custody without granting model or C/T/F authority
export class AdjustmentRainControlCustodyAnchorStore {
  #anchorRoot;
  #captureEpochPath;
  #expectedControlSha256;
  #expectedControlVersion;
  #readCaptureEpochWitness;
  #root;

  // retain only fixed evidence, witness and control identities
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#anchorRoot = join(this.#root, "rain-control-custody-anchors");
    this.#captureEpochPath = options.captureEpochPath ??
      ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH;
    this.#expectedControlSha256 = options.expectedControlSha256 ??
      process.env.WEATHER_ANCHOR_CONTROL_SHA256 ?? null;
    this.#expectedControlVersion = options.expectedControlVersion ??
      process.env.WEATHER_ANCHOR_CONTROL_VERSION ?? null;
    this.#readCaptureEpochWitness = options.readCaptureEpochWitness ??
      (() => readAdjustmentRevisionCaptureEpochWitness({
        expectedGid: 0,
        expectedUid: 0,
        path: this.#captureEpochPath,
      }));
  }

  // install one control-reference anchor after exact packed custody and epoch proof
  async install(bytesValue, expectedSha256) {
    requireSha256(expectedSha256, "adjustment rain control custody anchor SHA256");
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("adjustment rain control custody anchor bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const anchor = parseAdjustmentRainControlCustodyAnchor(bytes);
    // bind the operand and active control identity before state mutation
    if (sha256(bytes) !== expectedSha256 ||
      this.#expectedControlSha256 === null || this.#expectedControlVersion === null ||
      anchor.controlSha256 !== this.#expectedControlSha256 ||
      anchor.controlVersion !== this.#expectedControlVersion) {
      throw new Error("adjustment rain control custody anchor identity is invalid");
    }
    await requireRevisionCustodyDirectories(this.#root);
    const custodyState = await readRevisionCustodyAcknowledgements(this.#root);
    const custody = [custodyState.current, custodyState.previous].find(
      // select only the exact retained packed-custody acknowledgement
      (entry) => entry !== null &&
        entry.contractVersion === ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION &&
        entry.acknowledgementSha256 === anchor.custodyAcknowledgementSha256 &&
        entry.custodyCheckpointSha256 === anchor.custodyCheckpointSha256 &&
        entry.pageSha256 === anchor.pageSha256,
    );
    // crossbind every copied custody coordinate without granting qualification
    if (custody === undefined ||
      custody.acknowledgedAt !== anchor.custodyAcknowledgedAt ||
      custody.nextArchiveCommitOrdinal !== anchor.archiveCommitOrdinal ||
      custody.nextFrontierSha256 !== anchor.frontierSha256 ||
      custody.memberRootSha256 !== anchor.memberRootSha256 ||
      custody.startMemberSha256 !== anchor.startMemberSha256 ||
      custody.startSha256 !== anchor.startSha256) {
      throw new Error("adjustment rain control custody acknowledgement differs");
    }
    const witness = validateAdjustmentRevisionCaptureEpochWitness(
      await this.#readCaptureEpochWitness(),
    );
    // bind the state graph to the complete retained future-only epoch document
    if (sha256(canonicalJsonBytes(witness)) !== anchor.captureEpochWitnessSha256) {
      throw new Error("adjustment rain control custody epoch differs");
    }
    await this.#ensureRoot();
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    await this.#readSlot("previous");
    // converge only byte-identical installed retries
    if (current !== null && current.bytes.equals(bytes)) {
      if (pending !== null && !pending.bytes.equals(bytes)) {
        throw new Error("adjustment rain control custody pending collision");
      }
      if (pending !== null) {
        await unlink(join(this.#anchorRoot, "pending.json"));
        await fsyncDirectory(this.#anchorRoot);
      }
      return validateAdjustmentRainControlCustodyAnchorInstallation({
        anchorSha256: expectedSha256,
        contractVersion: "adjustment-rain-control-custody-anchor-installation/v1",
        state: "installed",
      });
    }
    const currentSha256 = current === null ? null : sha256(current.bytes);
    const expectedSequence = current === null ? 0n : BigInt(current.anchor.sequence) + 1n;
    // require an exact predecessor and monotonic sequence
    if (anchor.predecessorAnchorSha256 !== currentSha256 ||
      BigInt(anchor.sequence) !== expectedSequence) {
      throw new Error("adjustment rain control custody predecessor is invalid");
    }
    // preserve only one byte-identical in-progress transaction
    if (pending !== null) {
      if (!pending.bytes.equals(bytes)) {
        throw new Error("adjustment rain control custody pending collision");
      }
    } else {
      await writeExclusive(join(this.#anchorRoot, "pending.json"), bytes);
    }
    await this.#commitPending(expectedSha256);
    return validateAdjustmentRainControlCustodyAnchorInstallation({
      anchorSha256: expectedSha256,
      contractVersion: "adjustment-rain-control-custody-anchor-installation/v1",
      state: "installed",
    });
  }

  // return only the exact current control-reference anchor and its identity
  async readCurrent() {
    let details;
    try {
      details = await lstat(this.#anchorRoot, { bigint: true });
    } catch (error) {
      // represent only genuine absence as the empty create-once state
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    await this.#requireRoot(details);
    const current = await this.#readSlot("current");
    return validateAdjustmentRainControlCustodyAnchorCurrent(current === null ? null : {
      anchor: current.anchor,
      anchorSha256: sha256(current.bytes),
    });
  }

  // authorize only one exact inactive rain control-reference action
  async authorizeRainControlReferenceAction(input) {
    requireExactKeys(input, [
      "actionKind", "actionSha256", "controlStateSha256", "dueMonth",
      "fencingToken", "graphManifestSha256", "ordinalArtifactSha256",
      "sourceCommit", "sourceMemberRootSha256", "sourceReceiptRootSha256",
    ], "adjustment rain control authorization");
    if (input.actionKind !== "control_reference" ||
      !/^20\d{2}-(?:0[1-9]|1[0-2])$/u.test(input.dueMonth) ||
      !/^[a-f0-9]{40}$/u.test(input.sourceCommit)) {
      throw new TypeError("adjustment rain control authorization is invalid");
    }
    requireUint64Text(input.fencingToken,
      "adjustment rain control authorization fencingToken");
    for (const field of [
      "actionSha256", "controlStateSha256", "graphManifestSha256",
      "ordinalArtifactSha256", "sourceMemberRootSha256", "sourceReceiptRootSha256",
    ]) {
      requireSha256(input[field], `adjustment rain control authorization ${field}`);
    }
    const current = await this.readCurrent();
    // require exact control action authority without model or terminal aliases
    if (current === null || Object.entries(input).some(
      ([key, value]) => current.anchor[key] !== value,
    )) {
      throw new Error("adjustment rain control custody authorization differs");
    }
    return Object.freeze({ ...current.anchor });
  }

  // create only the fixed private rain control anchor root
  async #ensureRoot() {
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(this.#anchorRoot);
    await this.#requireRoot(await lstat(this.#anchorRoot, { bigint: true }));
  }

  // read one bounded rain control anchor slot without following links
  async #readSlot(slot) {
    const path = join(this.#anchorRoot, `${slot}.json`);
    let handle;
    try {
      handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    } catch (error) {
      // distinguish only genuine absence from unsafe slot state
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    try {
      const before = await handle.stat({ bigint: true });
      // require one bounded single-link owner-only slot
      if (!before.isFile() || before.uid !== BigInt(process.getuid()) ||
        before.gid !== BigInt(process.getgid()) || before.nlink !== 1n ||
        (before.mode & 0o777n) !== 0o600n || before.size < 1n ||
        before.size > BigInt(ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES)) {
        throw new Error("adjustment rain control custody slot is unsafe");
      }
      const bytes = await handle.readFile();
      const after = await handle.stat({ bigint: true });
      // reject replacement or mutation during the bounded read
      if (before.dev !== after.dev || before.ino !== after.ino ||
        before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
        before.ctimeNs !== after.ctimeNs) {
        throw new Error("adjustment rain control custody slot changed while reading");
      }
      return { anchor: parseAdjustmentRainControlCustodyAnchor(bytes), bytes };
    } finally {
      await handle.close();
    }
  }

  // commit one pending anchor while retaining only its direct predecessor
  async #commitPending(expectedSha256) {
    const pending = await this.#readSlot("pending");
    const current = await this.#readSlot("current");
    // accept one already committed retry after pending removal
    if (pending === null) {
      if (current !== null && sha256(current.bytes) === expectedSha256) {
        return;
      }
      throw new Error("adjustment rain control custody pending state is unavailable");
    }
    if (sha256(pending.bytes) !== expectedSha256) {
      throw new Error("adjustment rain control custody pending identity is invalid");
    }
    // retain only the direct predecessor before replacing current
    if (current !== null && sha256(current.bytes) !== expectedSha256) {
      await writePrivateAtomic(join(this.#anchorRoot, "previous.json"), current.bytes);
    }
    await writePrivateAtomic(join(this.#anchorRoot, "current.json"), pending.bytes);
    await unlink(join(this.#anchorRoot, "pending.json"));
    await fsyncDirectory(this.#anchorRoot);
  }

  // require one exact owner-private control anchor directory
  async #requireRoot(details) {
    if (!details.isDirectory() || details.isSymbolicLink() ||
      details.uid !== BigInt(process.getuid()) || details.gid !== BigInt(process.getgid()) ||
      (details.mode & 0o777n) !== 0o700n ||
      await realpath(this.#anchorRoot) !== this.#anchorRoot) {
      throw new Error("adjustment rain control custody root is unsafe");
    }
  }
}

// retain permanent unsupported custody without granting C, T, F or action authority
export class AdjustmentUnsupportedTerminalProofStore {
  #captureEpochPath;
  #expectedControlSha256;
  #expectedControlVersion;
  #proofRoot;
  #readCaptureEpochWitness;
  #readCurrentFamily;
  #root;

  // retain only fixed custody, epoch, control and live-source identities
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#proofRoot = join(this.#root, "unsupported-terminal-proofs");
    this.#captureEpochPath = options.captureEpochPath ??
      ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH;
    this.#expectedControlSha256 = options.expectedControlSha256 ??
      process.env.WEATHER_ANCHOR_CONTROL_SHA256 ?? null;
    this.#expectedControlVersion = options.expectedControlVersion ??
      process.env.WEATHER_ANCHOR_CONTROL_VERSION ?? null;
    this.#readCaptureEpochWitness = options.readCaptureEpochWitness ??
      (() => readAdjustmentRevisionCaptureEpochWitness({
        expectedGid: 0,
        expectedUid: 0,
        path: this.#captureEpochPath,
      }));
    this.#readCurrentFamily = options.readCurrentFamily ?? (async (family) => {
      const { readAdjustmentFamilyReleaseCurrent } =
        await import("./adjustment-evaluation-package.mjs");
      return await readAdjustmentFamilyReleaseCurrent(family);
    });
  }

  // install one unsupported proof after exact current custody and source verification
  async install(bytesValue, expectedSha256) {
    requireSha256(expectedSha256, "adjustment unsupported terminal proof SHA256");
    // reject implicit coercion at the privileged proof boundary
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("adjustment unsupported terminal proof bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const proof = parseAdjustmentUnsupportedTerminalProof(bytes);
    // bind the operand and active control identity before state mutation
    if (sha256(bytes) !== expectedSha256 ||
      this.#expectedControlSha256 === null || this.#expectedControlVersion === null ||
      proof.controlSha256 !== this.#expectedControlSha256 ||
      proof.controlVersion !== this.#expectedControlVersion) {
      throw new Error("adjustment unsupported terminal proof identity is invalid");
    }
    await requireRevisionCustodyDirectories(this.#root);
    const custodyState = await readRevisionCustodyAcknowledgements(this.#root);
    const custody = custodyState.current;
    // require the exact current v2 custody page rather than prior or semantic authority
    if (custody === null ||
      custody.contractVersion !== ADJUSTMENT_REVISION_CUSTODY_ACK_V2_CONTRACT_VERSION ||
      custody.custodyCheckpointSha256 !== proof.custodyCheckpointSha256 ||
      custody.pageSha256 !== proof.pageSha256 ||
      custody.nextArchiveCommitOrdinal !== proof.archiveCommitOrdinal ||
      custody.nextFrontierSha256 !== proof.frontierSha256 ||
      Date.parse(proof.fullGraphVerifiedAt) < Date.parse(custody.acknowledgedAt)) {
      throw new Error("adjustment unsupported terminal custody differs");
    }
    const witness = validateAdjustmentRevisionCaptureEpochWitness(
      await this.#readCaptureEpochWitness(),
    );
    // bind the proof to the complete immutable future-only epoch document
    if (sha256(canonicalJsonBytes(witness)) !== proof.captureEpochWitnessSha256) {
      throw new Error("adjustment unsupported terminal epoch differs");
    }
    const currentFamily = await this.#readCurrentFamily(proof.family);
    // bind terminal custody to the actual live family source commit
    if (currentFamily?.contractVersion !==
        "adjustment-family-release-current-status/v1" ||
      currentFamily.family !== proof.family || currentFamily.commit !== proof.sourceCommit) {
      throw new Error("adjustment unsupported terminal source differs");
    }
    await this.#ensureRoot();
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    await this.#readSlot("previous");
    // converge only byte-identical installed retries
    if (current !== null && current.bytes.equals(bytes)) {
      if (pending !== null && !pending.bytes.equals(bytes)) {
        throw new Error("adjustment unsupported terminal pending collision");
      }
      // finish one interrupted idempotent retry without replacing authority
      if (pending !== null) {
        await unlink(join(this.#proofRoot, "pending.json"));
        await fsyncDirectory(this.#proofRoot);
      }
      return validateAdjustmentUnsupportedTerminalProofInstallation({
        contractVersion:
          "adjustment-maintenance-unsupported-terminal-proof-installation/v1",
        proofSha256: expectedSha256,
        state: "installed",
      });
    }
    const currentSha256 = current === null ? null : sha256(current.bytes);
    const expectedSequence = current === null ? 0n : BigInt(current.proof.sequence) + 1n;
    // require an exact predecessor and monotonic bounded sequence
    if (proof.predecessorProofSha256 !== currentSha256 ||
      BigInt(proof.sequence) !== expectedSequence) {
      throw new Error("adjustment unsupported terminal predecessor is invalid");
    }
    // preserve only one byte-identical in-progress transaction
    if (pending !== null) {
      if (!pending.bytes.equals(bytes)) {
        throw new Error("adjustment unsupported terminal pending collision");
      }
    } else {
      await writeExclusive(join(this.#proofRoot, "pending.json"), bytes);
    }
    await this.#commitPending(expectedSha256);
    return validateAdjustmentUnsupportedTerminalProofInstallation({
      contractVersion: "adjustment-maintenance-unsupported-terminal-proof-installation/v1",
      proofSha256: expectedSha256,
      state: "installed",
    });
  }

  // return only the exact current unsupported proof and identity
  async readCurrent() {
    let details;
    try {
      details = await lstat(this.#proofRoot, { bigint: true });
    } catch (error) {
      // represent only genuine root absence as the empty state
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    await this.#requireRoot(details);
    await this.#requireFixedEntries();
    const current = await this.#readSlot("current");
    return validateAdjustmentUnsupportedTerminalProofCurrent(current === null ? null : {
      proof: current.proof,
      proofSha256: sha256(current.bytes),
    });
  }

  // create only the fixed owner-private unsupported proof root
  async #ensureRoot() {
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(this.#proofRoot);
    await this.#requireRoot(await lstat(this.#proofRoot, { bigint: true }));
    await this.#requireFixedEntries();
  }

  // reject any state outside the three fixed transaction slots
  async #requireFixedEntries() {
    const names = await readdir(this.#proofRoot);
    // prohibit foreign files from sharing the private authority root
    if (names.some((name) =>
      !["current.json", "pending.json", "previous.json"].includes(name))) {
      throw new Error("adjustment unsupported terminal root contains an unknown entry");
    }
  }

  // read one bounded unsupported proof slot without following links
  async #readSlot(slot) {
    const path = join(this.#proofRoot, `${slot}.json`);
    let handle;
    try {
      handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    } catch (error) {
      // distinguish only genuine absence from unsafe slot state
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    try {
      const before = await handle.stat({ bigint: true });
      // require one bounded single-link owner-only slot
      if (!before.isFile() || before.uid !== BigInt(process.getuid()) ||
        before.gid !== BigInt(process.getgid()) || before.nlink !== 1n ||
        (before.mode & 0o777n) !== 0o600n || before.size < 1n ||
        before.size > BigInt(ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES)) {
        throw new Error("adjustment unsupported terminal slot is unsafe");
      }
      const bytes = await handle.readFile();
      const after = await handle.stat({ bigint: true });
      // reject replacement or mutation during the bounded read
      if (before.dev !== after.dev || before.ino !== after.ino ||
        before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
        before.ctimeNs !== after.ctimeNs) {
        throw new Error("adjustment unsupported terminal slot changed while reading");
      }
      return { bytes, proof: parseAdjustmentUnsupportedTerminalProof(bytes) };
    } finally {
      await handle.close();
    }
  }

  // commit one pending proof while retaining only its direct predecessor
  async #commitPending(expectedSha256) {
    const pending = await this.#readSlot("pending");
    const current = await this.#readSlot("current");
    // accept one already committed retry after pending removal
    if (pending === null) {
      if (current !== null && sha256(current.bytes) === expectedSha256) {
        return;
      }
      throw new Error("adjustment unsupported terminal pending state is unavailable");
    }
    if (sha256(pending.bytes) !== expectedSha256) {
      throw new Error("adjustment unsupported terminal pending identity is invalid");
    }
    // retain only the direct predecessor before replacing current
    if (current !== null && sha256(current.bytes) !== expectedSha256) {
      await writePrivateAtomic(join(this.#proofRoot, "previous.json"), current.bytes);
    }
    await writePrivateAtomic(join(this.#proofRoot, "current.json"), pending.bytes);
    await unlink(join(this.#proofRoot, "pending.json"));
    await fsyncDirectory(this.#proofRoot);
  }

  // require one exact owner-private unsupported proof directory
  async #requireRoot(details) {
    if (!details.isDirectory() || details.isSymbolicLink() ||
      details.uid !== BigInt(process.getuid()) || details.gid !== BigInt(process.getgid()) ||
      (details.mode & 0o777n) !== 0o700n ||
      await realpath(this.#proofRoot) !== this.#proofRoot) {
      throw new Error("adjustment unsupported terminal root is unsafe");
    }
  }
}

// install only transferred cross-plane anchors without retirement authority
export class AdjustmentMaintenanceAnchorStore {
  #anchorRoot;
  #evidenceRoot;
  #expectedControlSha256;
  #expectedControlVersion;
  #sealStore;

  // retain fixed private roots only
  constructor(options = {}) {
    this.#evidenceRoot = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#anchorRoot = join(this.#evidenceRoot, "maintenance-anchors");
    this.#expectedControlSha256 = options.expectedControlSha256 ??
      process.env.WEATHER_ANCHOR_CONTROL_SHA256 ?? null;
    this.#expectedControlVersion = options.expectedControlVersion ??
      process.env.WEATHER_ANCHOR_CONTROL_VERSION ?? null;
    this.#sealStore = new AdjustmentFutureOnlyInputSealStore({
      captureEpochPath: options.captureEpochPath,
      expectedControlSha256: this.#expectedControlSha256,
      expectedControlVersion: this.#expectedControlVersion,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      readSourceClosure: options.readSourceClosure,
      root: this.#evidenceRoot,
    });
  }

  // read one exact current v3 anchor after active seal and source closure
  async readCurrentV3() {
    const installedSeal = await this.#sealStore.readCurrentVerified();
    let details;
    try {
      details = await lstat(this.#anchorRoot, { bigint: true });
    } catch (error) {
      // expose only genuine genesis absence as canonical null
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    await this.#requireRoot(details);
    const names = await readdir(this.#anchorRoot);
    // reject foreign state outside the fixed bounded transaction slots
    if (names.some((name) =>
      !["current.json", "pending.json", "previous.json"].includes(name))) {
      throw new Error("maintenance anchor root contains an unknown entry");
    }
    const current = await this.#readSlot("current");
    // preserve an initialized empty root as the same null genesis marker
    if (current === null) {
      return null;
    }
    const anchor = current.anchor;
    // expose only the future-only v3 grammar under active v14 control
    if (anchor.contractVersion !== ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION ||
      this.#expectedControlVersion !== "14" ||
      anchor.controlVersion !== this.#expectedControlVersion ||
      this.#expectedControlSha256 === null ||
      anchor.controlSha256 !== this.#expectedControlSha256) {
      throw new Error("future-only maintenance anchor active control differs");
    }
    // rebind the complete current v3 anchor to its exact retained current seal
    if (installedSeal === null || anchor.inputSealSha256 !== installedSeal.sealSha256 ||
      !futureAnchorMatchesSeal(anchor, installedSeal.seal) ||
      Date.parse(anchor.fullGraphVerifiedAt) < Date.parse(installedSeal.seal.sealedAt)) {
      throw new Error("future-only maintenance anchor current seal differs");
    }
    return validateAdjustmentMaintenanceAnchorCurrentV3({
      anchor,
      anchorSha256: sha256(current.bytes),
    });
  }

  // install one canonical T anchor after exact final-graph crossbinding
  async installTransferred(bytesValue, expectedSha256) {
    requireSha256(expectedSha256, "maintenance anchor SHA256");
    // reject implicit string or view coercion at the privileged boundary
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("maintenance anchor bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const anchor = parseMaintenanceAnchor(bytes);

    // bind the caller identity to the canonical bounded payload
    if (sha256(bytes) !== expectedSha256 || anchor.ctfState !== "transferred") {
      throw new Error("maintenance anchor identity is invalid");
    }
    // bind production installation to the active control-plane identity
    if ((this.#expectedControlSha256 !== null &&
        anchor.controlSha256 !== this.#expectedControlSha256) ||
      (this.#expectedControlVersion !== null &&
        anchor.controlVersion !== this.#expectedControlVersion)) {
      throw new Error("maintenance anchor control identity is invalid");
    }
    // select only the frozen future-only or legacy transfer authority
    let installedFutureSeal = null;
    if (anchor.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION) {
      installedFutureSeal = await this.#sealStore.readCurrent();
      // bind every server-sealed producer and custody identity into T
      if (installedFutureSeal === null ||
        installedFutureSeal.sealSha256 !== anchor.inputSealSha256 ||
        !futureAnchorMatchesSeal(anchor, installedFutureSeal.seal) ||
        Date.parse(anchor.fullGraphVerifiedAt) <
          Date.parse(installedFutureSeal.seal.sealedAt) ||
        Date.parse(anchor.publishedAt) < Date.parse(anchor.fullGraphVerifiedAt)) {
        throw new Error("future-only maintenance anchor seal binding is invalid");
      }
    } else {
      const ports = new AdjustmentCyclePageDiskPorts({ root: this.#evidenceRoot });
      const final = await ports.readFinalManifest(anchor.manifestSha256);
      const transfer = await ports.readFinalAcknowledgement(anchor.manifestSha256);

      // require the exact sealed generation and verified graph transfer receipt
      if (final === null || transfer === null || final.manifest.dueKey !== anchor.dueKey ||
        final.manifest.generation !== anchor.generation ||
        transfer.graphSha256 !== anchor.graphManifestSha256 ||
        Date.parse(anchor.fullGraphVerifiedAt) > Date.parse(transfer.acknowledgedAt) ||
        Date.parse(anchor.publishedAt) < Date.parse(transfer.acknowledgedAt)) {
        throw new Error("maintenance anchor transfer binding is invalid");
      }
    }
    await this.#ensureRoot();
    const current = await this.#readSlot("current");
    const pending = await this.#readSlot("pending");
    await this.#readSlot("previous");

    // finish an exact already-current retry without advancing sequence
    if (current !== null && current.bytes.equals(bytes)) {
      if (pending !== null && !pending.bytes.equals(bytes)) {
        throw new Error("maintenance anchor pending collision");
      }
      if (pending !== null) {
        await unlink(join(this.#anchorRoot, "pending.json"));
        await fsyncDirectory(this.#anchorRoot);
      }
      return anchor.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION
        ? validateAdjustmentMaintenanceAnchorInstallationV3({
            anchorSha256: expectedSha256,
            contractVersion: "adjustment-maintenance-anchor-installation/v3",
            state: "transferred",
          })
        : Object.freeze({ anchorSha256: expectedSha256, state: "transferred" });
    }
    // replay T only when current F is its exact authenticated finalization successor
    if (installedFutureSeal !== null && current !== null &&
      current.anchor.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION &&
      futureAnchorMatchesSeal(current.anchor, installedFutureSeal.seal) &&
      futureFinalizedAnchorSucceedsTransfer(current.anchor, anchor, expectedSha256)) {
      // preserve a different pending transaction rather than hiding concurrent drift
      if (pending !== null && !pending.bytes.equals(current.bytes)) {
        throw new Error("maintenance anchor pending collision");
      }
      // finish only a duplicate already-current F commit marker
      if (pending !== null) {
        await unlink(join(this.#anchorRoot, "pending.json"));
        await fsyncDirectory(this.#anchorRoot);
      }
      return validateAdjustmentMaintenanceAnchorInstallationV3({
        anchorSha256: expectedSha256,
        contractVersion: "adjustment-maintenance-anchor-installation/v3",
        state: "transferred",
      });
    }
    const currentSha256 = current === null ? null : sha256(current.bytes);
    const expectedSequence = current === null ? 0n : BigInt(current.anchor.sequence) + 1n;

    // require the next exact predecessor without reset or generation replay
    if (anchor.predecessorAnchorSha256 !== currentSha256 ||
      BigInt(anchor.sequence) !== expectedSequence ||
      (anchor.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_CONTRACT_VERSION &&
        current !== null && anchor.dueKey <= current.anchor.dueKey)) {
      throw new Error("maintenance anchor predecessor is invalid");
    }

    // preserve only one byte-identical in-progress transaction
    if (pending !== null) {
      if (!pending.bytes.equals(bytes)) {
        throw new Error("maintenance anchor pending collision");
      }
    } else {
      await writeExclusive(join(this.#anchorRoot, "pending.json"), bytes);
    }
    await this.#commitPending(expectedSha256);
    return anchor.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION
      ? validateAdjustmentMaintenanceAnchorInstallationV3({
          anchorSha256: expectedSha256,
          contractVersion: "adjustment-maintenance-anchor-installation/v3",
          state: "transferred",
        })
      : Object.freeze({ anchorSha256: expectedSha256, state: "transferred" });
  }

  // report bounded verified slot projections without claiming recovery or action authority
  async status() {
    let details;

    // treat only a wholly absent future root as not established
    try {
      details = await lstat(this.#anchorRoot, { bigint: true });
    } catch (error) {
      if (error?.code === "ENOENT") {
        return maintenanceAnchorStatus("unavailable", "unavailable", {});
      }
      throw error;
    }
    await this.#requireRoot(details);
    const names = (await readdir(this.#anchorRoot)).sort();

    // reject foreign state outside the three fixed transaction slots
    if (names.some((name) => !["current.json", "pending.json", "previous.json"].includes(name))) {
      throw new Error("maintenance anchor root contains an unknown entry");
    }
    const slots = {};

    // project each fixed slot through full schema validation
    for (const slot of ["current", "pending", "previous"]) {
      const value = await this.#readSlot(slot);
      slots[slot] = value === null
        ? { sha256: null, sizeBytes: null, state: "absent" }
        : { sha256: sha256(value.bytes), sizeBytes: value.bytes.length,
            state: value.anchor.ctfState };
    }
    return maintenanceAnchorStatus(
      "verified_private",
      names.length === 0 ? "unavailable" : "verified",
      slots,
    );
  }

  // finalize one root-bound cold proof before exact hot metadata retirement
  async finalizeRetirement(bytesValue, expectedSha256) {
    requireSha256(expectedSha256, "maintenance finalization proof SHA256");
    // reject implicit coercion at the privileged finalization boundary
    if (!Buffer.isBuffer(bytesValue)) {
      throw new TypeError("maintenance finalization proof bytes are invalid");
    }
    const bytes = Buffer.from(bytesValue);
    const proof = parseMaintenanceFinalizationProof(bytes);

    // bind the requested proof to its canonical bounded bytes
    if (sha256(bytes) !== expectedSha256) {
      throw new Error("maintenance finalization proof identity is invalid");
    }
    await this.#ensureRoot();
    const current = await this.#readSlot("current");

    // require one installed anchor as the only finalization authority
    if (current === null) {
      throw new Error("maintenance transferred anchor is unavailable");
    }
    // finalize future-only authority without relabelling it as legacy retirement
    if (current.anchor.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION) {
      return await this.#finalizeFutureOnly(
        current,
        proof,
        expectedSha256,
      );
    }
    // resume only the same already-committed F retirement transaction
    if (current.anchor.ctfState === "finalized") {
      if (current.anchor.finalizationProofSha256 !== expectedSha256) {
        throw new Error("maintenance finalization proof collision");
      }
      const retiredFiles = await retireMaintenanceHotFiles(
        this.#evidenceRoot,
        proof.retirementEntries,
      );
      return Object.freeze({
        anchorSha256: sha256(current.bytes),
        retiredFiles,
        retirementFileCount: proof.retirementEntries.length,
        status: "finalized",
      });
    }
    const transferredSha256 = sha256(current.bytes);
    const anchor = current.anchor;

    // bind every cold/catalog/controller identity to the installed T anchor
    validateFinalizationProofAgainstAnchor(proof, anchor, transferredSha256);
    const ports = new AdjustmentCyclePageDiskPorts({ root: this.#evidenceRoot });
    const final = await ports.readFinalManifest(anchor.manifestSha256);
    const transfer = await ports.readFinalAcknowledgement(anchor.manifestSha256);

    // require the retained sealed final and its exact full-graph acknowledgement
    if (final === null || transfer === null || final.manifest.inputSeal === undefined ||
      transfer.graphSha256 !== anchor.graphManifestSha256) {
      throw new Error("maintenance finalization transfer proof is unavailable");
    }
    const inputSealSha256 = sha256(canonicalJsonBytes(final.manifest.inputSeal));
    const requiredInputRootSha256 = sha256(canonicalJsonBytes(
      final.manifest.inputSeal.requiredInputs,
    ));
    const requiredInputs = final.manifest.inputSeal.requiredInputs;

    // require every genuine revision and shadow producer before F or burn
    if (["actual_best_match", "native_source", "rain_gate_input", "shadow_body",
      "shadow_metadata", "target_revision"].some(
      (kind) => requiredInputs[kind].count < 1,
    )) {
      throw new Error("maintenance finalization producer population is incomplete");
    }
    const revisionInputCount = ["actual_best_match", "native_source", "rain_gate_input",
      "target_revision"].reduce(
      // count every server-ordinal revision class in the sealed cycle
      (total, kind) => total + requiredInputs[kind].count,
      0,
    );
    // prohibit a frontier too small to contain the sealed revision population
    if (proof.frontierCount < revisionInputCount) {
      throw new Error("maintenance finalization revision frontier is incomplete");
    }
    // crossbind the server-sealed producer population to the cold proof
    if (proof.inputSealSha256 !== inputSealSha256 ||
      proof.requiredInputRootSha256 !== requiredInputRootSha256) {
      throw new Error("maintenance finalization input seal differs");
    }
    const expectedEntries = await collectMaintenanceRetirementEntries(
      this.#evidenceRoot,
      ports,
      final,
    );

    // require exact hash-bound redundant cold members for every retired file
    if (canonicalJson(proof.retirementEntries) !== canonicalJson(expectedEntries) ||
      proof.retirementSetRootSha256 !== sha256(canonicalJsonBytes(expectedEntries))) {
      throw new Error("maintenance finalization retirement set differs");
    }
    const finalized = {
      ...anchor,
      archiveCommitOrdinal: proof.archiveCommitOrdinal,
      burnSha256: proof.burnSha256,
      catalogRevisionReceiptRootSha256: proof.catalogRevisionReceiptRootSha256,
      confirmationChunkCount: proof.confirmationChunkCount,
      ctfState: "finalized",
      finalizationProofSha256: expectedSha256,
      finalizedAt: proof.finalizedAt,
      inputSealSha256: proof.inputSealSha256,
      requiredInputRootSha256: proof.requiredInputRootSha256,
      retirementSetRootSha256: proof.retirementSetRootSha256,
    };
    const finalizedBytes = canonicalJsonBytes(finalized);
    parseMaintenanceAnchor(finalizedBytes);
    const finalizedSha256 = sha256(finalizedBytes);
    await writeExclusive(join(this.#anchorRoot, "pending.json"), finalizedBytes);
    await this.#commitPending(finalizedSha256);
    const retiredFiles = await retireMaintenanceHotFiles(
      this.#evidenceRoot,
      proof.retirementEntries,
    );
    return Object.freeze({
      anchorSha256: finalizedSha256,
      retiredFiles,
      retirementFileCount: proof.retirementEntries.length,
      status: "finalized",
    });
  }

  // authorize development publication without granting model action authority
  async authorizeShadowCandidate(input) {
    requireExactKeys(input, [
      "catalogRootSha256", "controlSha256", "controlVersion",
      "graphManifestSha256", "sourceCommit",
    ], "shadow candidate authorization");
    const current = await this.#readSlot("current");

    // require one installed T or F root with exact immutable graph identities
    if (current === null || !["transferred", "finalized"].includes(current.anchor.ctfState) ||
      input.catalogRootSha256 !== current.anchor.catalogRootSha256 ||
      input.controlSha256 !== current.anchor.controlSha256 ||
      input.controlVersion !== current.anchor.controlVersion ||
      input.graphManifestSha256 !== current.anchor.graphManifestSha256 ||
      input.sourceCommit !== current.anchor.sourceCommit) {
      throw new Error("shadow candidate anchor authorization differs");
    }
    return Object.freeze({ ...current.anchor });
  }

  // authorize one qualified action only from the exact finalized burn anchor
  async authorizeQualifiedAction(input) {
    requireExactKeys(input, [
      "actionSha256", "burnSha256", "controlSha256", "controlVersion",
      "fullMemberRootSha256", "lifecycleLedgerRootSha256", "reportSha256",
      "revisionSnapshotRootSha256", "sourceCommit", "workstationJournalHeadSha256",
    ], "qualified action authorization");
    const current = await this.#readSlot("current");

    // require the installed F anchor to bind every proposed action identity
    if (current === null || current.anchor.ctfState !== "finalized" ||
      Object.entries(input).some(([key, value]) => current.anchor[key] !== value)) {
      throw new Error("qualified action anchor authorization differs");
    }
    return Object.freeze({ ...current.anchor });
  }

  // authorize one future-only qualified action from the exact finalized v3 anchor
  async authorizeFutureOnlyQualifiedAction(input) {
    requireExactKeys(input, [
      "actionSha256", "fullMemberRootSha256", "lifecycleLedgerRootSha256",
      "policyReportSha256", "sourceCommit",
    ], "future-only qualified action authorization");
    requireSha256(input.actionSha256, "future-only actionSha256");
    requireSha256(input.fullMemberRootSha256, "future-only fullMemberRootSha256");
    requireSha256(input.lifecycleLedgerRootSha256,
      "future-only lifecycleLedgerRootSha256");
    requireSha256(input.policyReportSha256, "future-only policyReportSha256");
    // retain the exact public source commit grammar at the final action boundary
    if (typeof input.sourceCommit !== "string" ||
      !/^[a-f0-9]{40}$/u.test(input.sourceCommit)) {
      throw new TypeError("future-only sourceCommit is invalid");
    }
    const current = await this.#readSlot("current");
    // require F and every action-carried authority identity without legacy aliases
    if (current === null ||
      current.anchor.contractVersion !== ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION ||
      current.anchor.ctfState !== "finalized" ||
      Object.entries(input).some(([key, value]) => current.anchor[key] !== value)) {
      throw new Error("future-only qualified action anchor authorization differs");
    }
    return Object.freeze({ ...current.anchor });
  }

  // transition one exact future-only T anchor to F without hot-file authority
  async #finalizeFutureOnly(current, proof, expectedSha256) {
    const anchor = current.anchor;
    // converge only the exact already-finalized proof identity
    if (anchor.ctfState === "finalized") {
      if (anchor.finalizationProofSha256 !== expectedSha256) {
        throw new Error("future-only maintenance finalization proof collision");
      }
      return validateAdjustmentMaintenanceAnchorFinalizationV3({
        anchorSha256: sha256(current.bytes),
        contractVersion: "adjustment-maintenance-anchor-finalization/v3",
        finalizationProofSha256: expectedSha256,
        state: "finalized",
      });
    }
    // bind the proof to the exact transferred anchor and every T identity
    if (proof.contractVersion !== ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_V3_CONTRACT_VERSION ||
      proof.transferredAnchorSha256 !== sha256(current.bytes) ||
      !futureFinalizationMatchesAnchor(proof, anchor)) {
      throw new Error("future-only maintenance finalization proof differs");
    }
    const finalized = validateAdjustmentMaintenanceAnchorV3({
      ...anchor,
      ctfState: "finalized",
      finalizationProofSha256: expectedSha256,
      finalizedAt: proof.finalizedAt,
    });
    const finalizedBytes = canonicalJsonBytes(finalized);
    const pending = await this.#readSlot("pending");
    // resume only a byte-identical pending future-only finalization
    if (pending !== null) {
      if (!pending.bytes.equals(finalizedBytes)) {
        throw new Error("maintenance anchor pending collision");
      }
    } else {
      await writeExclusive(join(this.#anchorRoot, "pending.json"), finalizedBytes);
    }
    await this.#commitPending(sha256(finalizedBytes));
    return validateAdjustmentMaintenanceAnchorFinalizationV3({
      anchorSha256: sha256(finalizedBytes),
      contractVersion: "adjustment-maintenance-anchor-finalization/v3",
      finalizationProofSha256: expectedSha256,
      state: "finalized",
    });
  }

  // create only the fixed owner-private anchor root
  async #ensureRoot() {
    await requirePrivateDirectory(this.#evidenceRoot);
    await ensurePrivateDirectory(this.#anchorRoot);
    await this.#requireRoot(await lstat(this.#anchorRoot, { bigint: true }));
  }

  // read one root-owner slot without following links
  async #readSlot(slot) {
    const path = join(this.#anchorRoot, `${slot}.json`);
    let handle;

    // distinguish only genuine absence from unsafe slot state
    try {
      handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    } catch (error) {
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    try {
      const before = await handle.stat({ bigint: true });

      // require one bounded single-link file owned by the installing identity
      if (!before.isFile() || before.uid !== BigInt(process.getuid()) ||
        before.gid !== BigInt(process.getgid()) || before.nlink !== 1n ||
        (before.mode & 0o777n) !== 0o600n || before.size < 1n ||
        before.size > BigInt(ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES)) {
        throw new Error("maintenance anchor slot is unsafe");
      }
      const bytes = await handle.readFile();
      const after = await handle.stat({ bigint: true });

      // reject replacement or mutation during the bounded read
      if (before.dev !== after.dev || before.ino !== after.ino ||
        before.size !== after.size || before.mtimeNs !== after.mtimeNs ||
        before.ctimeNs !== after.ctimeNs) {
        throw new Error("maintenance anchor slot changed while reading");
      }
      return { anchor: parseMaintenanceAnchor(bytes), bytes };
    } finally {
      await handle.close();
    }
  }

  // finish one pending transaction with recoverable previous/current rotation
  async #commitPending(expectedSha256) {
    const pending = await this.#readSlot("pending");
    const current = await this.#readSlot("current");

    // accept only the exact pending payload or an already committed retry
    if (pending === null) {
      if (current !== null && sha256(current.bytes) === expectedSha256) {
        return;
      }
      throw new Error("maintenance anchor pending state is unavailable");
    }
    if (sha256(pending.bytes) !== expectedSha256) {
      throw new Error("maintenance anchor pending identity is invalid");
    }

    // preserve the previous committed anchor before replacing current
    if (current !== null && sha256(current.bytes) !== expectedSha256) {
      await writePrivateAtomic(join(this.#anchorRoot, "previous.json"), current.bytes);
    }
    await writePrivateAtomic(join(this.#anchorRoot, "current.json"), pending.bytes);
    await unlink(join(this.#anchorRoot, "pending.json"));
    await fsyncDirectory(this.#anchorRoot);
  }

  // require exact private ownership for the root control boundary
  async #requireRoot(details) {
    // reject links, aliases, foreign owners and broad modes
    if (!details.isDirectory() || details.isSymbolicLink() ||
      details.uid !== BigInt(process.getuid()) || details.gid !== BigInt(process.getgid()) ||
      (details.mode & 0o777n) !== 0o700n ||
      await realpath(this.#anchorRoot) !== this.#anchorRoot) {
      throw new Error("maintenance anchor root is unsafe");
    }
  }
}

// keep capture errors outside the response path
export class AdjustmentEvidenceStore {
  #allocatedBytes = 0;
  #blockedIdentities = new Set();
  #compressionCache = new Map();
  #initialized = false;
  #objectCount = 0;
  #options;
  #queue = Promise.resolve();
  #receiptCount = 0;
  #status = {
    asyncErrors: 0,
    asyncGaps: 0,
    bytes: 0,
    capacityExhausted: 0,
    channelAssertions: 0,
    channelCollisions: 0,
    closeWithoutFinish: 0,
    collisions: 0,
    finishedSuccesses: 0,
    freeSpaceRefusal: 0,
    inodeRefusal: 0,
    objectTooLarge: 0,
    objects: 0,
    provenanceIncomplete: 0,
  };

  // retain injectable boundaries for deterministic fault tests
  constructor(options = {}) {
    this.#options = {
      beforeObjectWrite: options.beforeObjectWrite ?? (() => undefined),
      compressObject: options.compressObject ?? ((bytes) =>
        gzipSync(bytes, { level: 9, mtime: 0 })),
      now: options.now ?? (() => new Date()),
      objectSha256: options.objectSha256 ?? sha256,
      root: resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT),
      statfs: options.statfs ?? ((path) => statfs(path, { bigint: true })),
      writeExclusive: options.writeExclusive ?? writeExclusive,
    };
  }

  // create and scan the bounded immutable store
  async initialize() {
    try {
      await ensureEvidenceDirectories(this.#options.root);
      const scan = await scanEvidenceDirectories(this.#options.root, true);
      this.#allocatedBytes = scan.allocatedBytes;
      this.#objectCount = scan.objectCount;
      this.#receiptCount = scan.receiptCount;
      this.#status.channelAssertions = scan.channelAssertionCount;
      this.#status.bytes = scan.allocatedBytes;
      this.#status.objects = scan.objectCount;
      this.#initialized = true;
    } catch {
      this.#status.asyncErrors += 1;
      this.#initialized = false;
    }
    return this;
  }

  // derive immutable capture bytes before writing the response
  prepare(filteredBody, window) {
    return this.#prepareCapture(() => createAdjustmentEvidenceCaptureFromForecast(
      JSON.parse(Buffer.from(filteredBody).toString("utf8")),
      window,
      (canonicalObject) => this.#compressCanonicalObject(canonicalObject),
    ));
  }

  // derive immutable capture bytes from one already parsed forecast
  prepareForecast(forecast, window) {
    return this.#prepareCapture(() => createAdjustmentEvidenceCaptureFromForecast(
      forecast,
      window,
      (canonicalObject) => this.#compressCanonicalObject(canonicalObject),
    ));
  }

  // derive one v2 capture with a trusted server-selected channel
  prepareV2(filteredBody, window, channel) {
    return this.#prepareCapture(() => this.#createV2Capture(
      JSON.parse(Buffer.from(filteredBody).toString("utf8")),
      window,
      channel,
    ));
  }

  // derive one v2 capture from one already parsed forecast
  prepareForecastV2(forecast, window, channel) {
    return this.#prepareCapture(() => this.#createV2Capture(forecast, window, channel));
  }

  // contain one synchronous capture refusal
  #prepareCapture(createCapture) {
    // refuse capture while filesystem integrity is unknown
    if (!this.#initialized) {
      this.#status.asyncGaps += 1;
      return null;
    }

    try {
      return createCapture();
    } catch (error) {
      // classify only bounded projection failures
      if (error?.code === "adjustment_evidence_object_too_large") {
        this.#status.objectTooLarge += 1;
      } else {
        this.#status.asyncErrors += 1;
      }
      this.#status.asyncGaps += 1;
      return null;
    }
  }

  // derive one channel-bound capture without reparsing its forecast
  #createV2Capture(forecast, window, channel) {
    requireIssuanceChannel(channel);
    const capture = forecast?.data?.length === MAXIMUM_V2_ROWS
      ? createAdjustmentEvidenceCaptureV2FromForecast(
          forecast,
          window,
          (canonicalObject) => this.#compressCanonicalObject(canonicalObject),
        )
      : createAdjustmentEvidenceCaptureFromForecast(
          forecast,
          window,
          (canonicalObject) => this.#compressCanonicalObject(canonicalObject),
        );
    return Object.freeze({
      ...capture,
      issuanceChannel: channel,
    });
  }

  // reuse only identical canonical bytes under one object hash
  #compressCanonicalObject(canonicalObject) {
    const objectSha256 = this.#options.objectSha256(canonicalObject);
    requireSha256(objectSha256, "adjustment evidence object");
    const cached = this.#compressionCache.get(objectSha256);

    // refuse one same-hash canonical-byte collision without promotion
    if (cached !== undefined && !cached.canonicalObject.equals(canonicalObject)) {
      throw new Error("adjustment evidence compression cache collision");
    }

    // return one detached hit and promote its owned entry
    if (cached !== undefined) {
      this.#compressionCache.delete(objectSha256);
      this.#compressionCache.set(objectSha256, cached);
      return {
        compressedObject: Buffer.from(cached.compressedObject),
        objectSha256,
      };
    }
    const compressedObject = this.#options.compressObject(canonicalObject);
    validateCompressedEvidenceObject(canonicalObject, compressedObject);
    this.#compressionCache.set(objectSha256, {
      canonicalObject: Buffer.from(canonicalObject),
      compressedObject: Buffer.from(compressedObject),
    });

    // evict the deterministic least-recent entry
    if (this.#compressionCache.size > COMPRESSION_CACHE_MAXIMUM_ENTRIES) {
      const oldest = this.#compressionCache.keys().next().value;
      this.#compressionCache.delete(oldest);
    }
    return { compressedObject: Buffer.from(compressedObject), objectSha256 };
  }

  // attach persistence only to Node's successful finish event
  trackResponse(response, prepared, onSettled = () => undefined) {
    let finished = false;

    // persist asynchronously only after the response commits
    response.once("finish", () => {
      finished = true;

      // retain one explicit capture gap when preparation failed
      if (prepared === null) {
        onSettled({ status: "preparation_refused" });
        return;
      }

      const firstEdgeCommittedAt = this.#options.now().toISOString();
      void this.commit(prepared, firstEdgeCommittedAt).then(
        // disclose the post-finish result only to the in-process caller
        (result) => onSettled(result),
        // contain all filesystem failures after response completion
        (error) => {
          this.#status.asyncErrors += 1;
          this.#status.asyncGaps += 1;
          onSettled({ status: "write_failed", error });
        },
      );
    });

    // distinguish aborted sockets from successfully finished responses
    response.once("close", () => {
      if (!finished) {
        this.#status.closeWithoutFinish += 1;
        onSettled({ status: "response_aborted" });
      }
    });
  }

  // serialize exclusive writes and capacity accounting
  async commit(prepared, firstEdgeCommittedAt) {
    const operation = this.#queue.then(
      // keep one writer inside the allocation guard
      () => this.#commitExclusive(prepared, firstEdgeCommittedAt),
    );
    this.#queue = operation.catch(() => undefined);
    return await operation;
  }

  // return bounded in-memory health counters only
  status() {
    return Object.freeze({ ...this.#status });
  }

  // write one content object and one first-issuance receipt
  async #commitExclusive(prepared, firstEdgeCommittedAt) {
    requireInstant(firstEdgeCommittedAt, "firstEdgeCommittedAt");

    // stop prepared work after any integrity-threatening write failure
    if (!this.#initialized) {
      this.#status.asyncGaps += 1;
      return { status: "store_unavailable" };
    }

    // stop a known collision identity without attempting repair
    if (this.#blockedIdentities.has(prepared.edgeReceiptIdentitySha256)) {
      this.#status.asyncGaps += 1;
      return { status: "identity_collision" };
    }

    const objectPath = evidenceObjectPath(this.#options.root, prepared.objectSha256);
    const receiptPath = evidenceReceiptPath(
      this.#options.root,
      prepared.edgeReceiptIdentitySha256,
    );
    const receiptExists = await regularFileExists(receiptPath);

    // honor an existing first receipt before creating any content object
    if (receiptExists) {
      const existingBytes = await readRegularFile(receiptPath, RECEIPT_MAXIMUM_BYTES);
      const existing = parseReceipt(existingBytes, prepared.edgeReceiptIdentitySha256);

      // block a stable identity already bound to different scoring content
      if (existing.objectSha256 !== prepared.objectSha256) {
        this.#blockedIdentities.add(prepared.edgeReceiptIdentitySha256);
        this.#status.collisions += 1;
        this.#status.asyncGaps += 1;
        await this.#refreshAllocationFromDisk();
        return { status: "identity_collision" };
      }

      const existingObject = await readRegularFile(
        objectPath,
        COMPRESSED_OBJECT_MAXIMUM_BYTES,
      );

      // require the receipt's immutable object to remain byte-identical
      if (!existingObject.equals(prepared.compressedObject)) {
        throw new Error("adjustment evidence receipt object is invalid");
      }

      await this.#commitChannelAssertion(prepared, existing, existingBytes, true);
      await this.#refreshAllocationFromDisk();
      return { status: "duplicate" };
    }

    const objectExists = await regularFileExists(objectPath);
    const receipt = createReceipt(prepared, firstEdgeCommittedAt);
    const receiptBytes = Buffer.from(`${canonicalJson(receipt)}\n`);
    const receiptMaximumBytes = receipt.contractVersion ===
      ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION
      ? 2_048
      : RECEIPT_MAXIMUM_BYTES;

    // enforce the independent receipt bound before filesystem work
    if (receiptBytes.byteLength > receiptMaximumBytes) {
      this.#status.objectTooLarge += 1;
      this.#status.asyncGaps += 1;
      return { status: "object_too_large" };
    }

    const filesystem = await this.#options.statfs(this.#options.root);
    const blockSize = Number(filesystem.bsize);
    const prospectiveBytes =
      (objectExists ? 0 : allocatedSize(prepared.compressedObject.byteLength, blockSize)) +
      (receiptExists ? 0 : allocatedSize(receiptBytes.byteLength, blockSize));

    // keep identities and allocated bytes inside the frozen envelope
    if (
      (!objectExists && this.#objectCount >= MAXIMUM_IDENTITIES) ||
      (!receiptExists && this.#receiptCount >= MAXIMUM_IDENTITIES) ||
      this.#allocatedBytes + prospectiveBytes > LEDGER_ALLOCATION_MAXIMUM_BYTES
    ) {
      this.#status.capacityExhausted += 1;
      this.#status.asyncGaps += 1;
      return { status: "capacity_exhausted" };
    }

    const freeBytes = Number(filesystem.bavail * filesystem.bsize);
    const freeInodes = Number(filesystem.ffree);

    // preserve the protected floor and next-capture reservation
    if (freeBytes - prospectiveBytes <
      ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES) {
      this.#status.freeSpaceRefusal += 1;
      this.#status.asyncGaps += 1;
      return { status: "capacity_exhausted" };
    }

    // retain enough free inodes for host operations
    if (freeInodes < FREE_INODE_FLOOR) {
      this.#status.inodeRefusal += 1;
      this.#status.asyncGaps += 1;
      return { status: "capacity_exhausted" };
    }

    let objectCreated;

    try {
      await this.#options.beforeObjectWrite();
      objectCreated = await writeOrVerifyImmutable(
        objectPath,
        prepared.compressedObject,
        COMPRESSED_OBJECT_MAXIMUM_BYTES,
        this.#options.writeExclusive,
      );
    } catch (error) {
      // reconcile any complete or injected partial final before disabling capture
      await this.#reconcileWriteFailure();
      throw error;
    }

    // account an object before any receipt race or failure can return
    this.#applyCreatedAllocation(objectCreated, false, prepared, receiptBytes, blockSize);

    let receiptCreated;

    try {
      receiptCreated = await writeOrVerifyReceipt(
        receiptPath,
        receiptBytes,
        prepared,
        this.#options.writeExclusive,
      );
    } catch (error) {
      // retain an orphan immutable object as an explicit gap
      if (objectCreated) {
        this.#status.asyncGaps += 1;
      }
      await this.#reconcileWriteFailure();
      throw error;
    }

    // a concurrent first writer may have bound a different object
    if (!receiptCreated) {
      const existing = await readReceipt(receiptPath, prepared.edgeReceiptIdentitySha256);

      if (existing.objectSha256 !== prepared.objectSha256) {
        this.#blockedIdentities.add(prepared.edgeReceiptIdentitySha256);
        this.#status.collisions += 1;
        this.#status.asyncGaps += 1;
        await this.#refreshAllocationFromDisk();
        return { status: "identity_collision" };
      }
    }

    this.#applyCreatedAllocation(
      false,
      receiptCreated,
      prepared,
      receiptBytes,
      blockSize,
    );

    // count only the writer that retained the first receipt
    if (receiptCreated) {
      // reconcile an object published by another writer after our last scan
      if (!objectCreated) {
        await this.#refreshAllocationFromDisk();
      }
      this.#status.finishedSuccesses += 1;
      this.#status.provenanceIncomplete += prepared.object.rows.length;
      await this.#commitChannelAssertion(prepared, receipt, receiptBytes, false);
      await this.#refreshAllocationFromDisk();
      return { status: "created" };
    }

    const existingBytes = await readRegularFile(receiptPath, RECEIPT_MAXIMUM_BYTES);
    const existingReceipt = parseReceipt(
      existingBytes,
      prepared.edgeReceiptIdentitySha256,
    );
    await this.#commitChannelAssertion(prepared, existingReceipt, existingBytes, true);
    await this.#refreshAllocationFromDisk();
    return { status: "duplicate" };
  }

  // bind the first trusted issuance channel without changing v1 bytes
  async #commitChannelAssertion(prepared, receipt, receiptBytes, receiptWasExisting) {
    // leave legacy callers byte-identical and unattributed
    if (prepared.issuanceChannel === undefined) {
      return;
    }

    requireIssuanceChannel(prepared.issuanceChannel);
    const bindingsPath = join(
      this.#options.root,
      "channel-bindings",
      `sha256-${prepared.edgeReceiptIdentitySha256}.json`,
    );
    const existingBinding = await readOptionalPrivateFile(bindingsPath, RECEIPT_MAXIMUM_BYTES);

    // preserve the first channel selected by trusted process state
    if (existingBinding !== null) {
      const binding = parseChannelBinding(
        existingBinding,
        prepared.edgeReceiptIdentitySha256,
      );
      const assertionBytes = await readRegularFile(
        join(this.#options.root, "channels", `sha256-${binding.assertionSha256}.json`),
        RECEIPT_MAXIMUM_BYTES,
      );
      const assertion = parseChannelAssertion(assertionBytes, binding.assertionSha256);

      // reject a binding substituted for another immutable receipt
      if (assertion.edgeReceiptIdentitySha256 !== prepared.edgeReceiptIdentitySha256 ||
        assertion.receiptSha256 !== sha256(receiptBytes) ||
        assertion.firstEdgeCommittedAt !== receipt.firstEdgeCommittedAt) {
        this.#status.channelCollisions += 1;
        throw new Error("adjustment evidence channel binding collision");
      }
      return;
    }

    const assertion = {
      channel: receiptWasExisting ? "legacy_unattributed" : prepared.issuanceChannel,
      contractVersion: ADJUSTMENT_EVIDENCE_CHANNEL_CONTRACT_VERSION,
      edgeReceiptIdentitySha256: prepared.edgeReceiptIdentitySha256,
      firstEdgeCommittedAt: receipt.firstEdgeCommittedAt,
      receiptSha256: sha256(receiptBytes),
    };
    const assertionBytes = Buffer.from(`${canonicalJson(assertion)}\n`);

    // keep each closed assertion within the independent receipt ceiling
    if (assertionBytes.byteLength > 2_048) {
      throw objectTooLargeError();
    }

    const assertionSha256 = sha256(assertionBytes);
    const assertionPath = join(
      this.#options.root,
      "channels",
      `sha256-${assertionSha256}.json`,
    );
    const binding = {
      assertionSha256,
      contractVersion: "issuance-channel-binding/v2",
      edgeReceiptIdentitySha256: prepared.edgeReceiptIdentitySha256,
    };
    const bindingBytes = Buffer.from(`${canonicalJson(binding)}\n`);
    const [bindingNames, channelNames] = await Promise.all([
      readdir(join(this.#options.root, "channel-bindings")),
      readdir(join(this.#options.root, "channels")),
    ]);
    const assertionExists = await regularFileExists(assertionPath);

    // retain the exact bounded hot assertion ceiling without pruning
    if (bindingNames.length >= MAXIMUM_CHANNEL_ASSERTIONS ||
      (!assertionExists && channelNames.length >= MAXIMUM_CHANNEL_ASSERTIONS)) {
      this.#status.capacityExhausted += 1;
      throw new Error("adjustment evidence channel capacity is exhausted");
    }

    const filesystem = await this.#options.statfs(this.#options.root);
    const blockSize = Number(filesystem.bsize);
    const prospectiveBytes = (assertionExists
      ? 0
      : allocatedSize(assertionBytes.byteLength, blockSize)) +
      allocatedSize(bindingBytes.byteLength, blockSize);
    const freeBytes = Number(filesystem.bavail * filesystem.bsize);

    // preserve the immutable ledger allocation ceiling
    if (this.#allocatedBytes + prospectiveBytes > LEDGER_ALLOCATION_MAXIMUM_BYTES) {
      this.#status.capacityExhausted += 1;
      throw new Error("adjustment evidence channel capacity is exhausted");
    }

    // preserve the protected floor and next-capture reservation
    if (freeBytes - prospectiveBytes < ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES) {
      this.#status.freeSpaceRefusal += 1;
      throw new Error("adjustment evidence channel capacity is exhausted");
    }

    // preserve the shared free-inode floor for two new files
    if (Number(filesystem.ffree) < FREE_INODE_FLOOR) {
      this.#status.inodeRefusal += 1;
      throw new Error("adjustment evidence channel capacity is exhausted");
    }

    await writeOrVerifyImmutable(
      assertionPath,
      assertionBytes,
      2_048,
      this.#options.writeExclusive,
    );

    try {
      let created = true;

      try {
        await this.#options.writeExclusive(bindingsPath, bindingBytes);
      } catch (error) {
        // preserve a concurrently installed first trusted channel
        if (error?.code !== "EEXIST") {
          throw error;
        }
        created = false;
        const concurrentBytes = await readRegularFile(bindingsPath, RECEIPT_MAXIMUM_BYTES);
        const concurrent = parseChannelBinding(
          concurrentBytes,
          prepared.edgeReceiptIdentitySha256,
        );
        const concurrentAssertionBytes = await readRegularFile(
          join(this.#options.root, "channels", `sha256-${concurrent.assertionSha256}.json`),
          RECEIPT_MAXIMUM_BYTES,
        );
        const concurrentAssertion = parseChannelAssertion(
          concurrentAssertionBytes,
          concurrent.assertionSha256,
        );

        // reject only a binding for different immutable receipt bytes
        if (concurrentAssertion.receiptSha256 !== sha256(receiptBytes) ||
          concurrentAssertion.firstEdgeCommittedAt !== receipt.firstEdgeCommittedAt) {
          throw new Error("adjustment evidence channel binding collision");
        }
      }

      // count only one newly retained stable binding
      if (created) {
        this.#status.channelAssertions += 1;
      }
    } catch (error) {
      this.#status.channelCollisions += 1;
      throw error;
    }
  }

  // refresh global immutable allocation after a cross-writer observation
  async #refreshAllocationFromDisk() {
    const scan = await scanEvidenceDirectories(this.#options.root, false);
    this.#allocatedBytes = scan.allocatedBytes;
    this.#objectCount = scan.objectCount;
    this.#receiptCount = scan.receiptCount;
    this.#status.bytes = scan.allocatedBytes;
    this.#status.objects = scan.objectCount;
    this.#status.channelAssertions = scan.channelAssertionCount;
  }

  // account retained finals and stop further capture after a write fault
  async #reconcileWriteFailure() {
    try {
      await this.#refreshAllocationFromDisk();
    } finally {
      this.#initialized = false;
    }
  }

  // apply only allocations created by this process
  #applyCreatedAllocation(objectCreated, receiptCreated, prepared, receiptBytes, blockSize) {
    // update immutable object allocation once
    if (objectCreated) {
      this.#objectCount += 1;
      this.#allocatedBytes += allocatedSize(prepared.compressedObject.byteLength, blockSize);
    }

    // update immutable receipt allocation once
    if (receiptCreated) {
      this.#receiptCount += 1;
      this.#allocatedBytes += allocatedSize(receiptBytes.byteLength, blockSize);
    }

    this.#status.bytes = this.#allocatedBytes;
    this.#status.objects = this.#objectCount;
  }
}

// persist bounded scheduler restart state under the existing private root
export class AdjustmentEvidenceSchedulerStateStore {
  #root;

  // retain one injectable private state root
  constructor(options = {}) {
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
  }

  // create only the unprivileged scheduler state directory
  async initialize() {
    const path = join(this.#root, "scheduler");
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(path);
    return this;
  }

  // recover current or previous validated state after a torn replacement
  async read() {
    let invalidObserved = false;

    for (const name of ["current.json", "previous.json"]) {
      try {
        const bytes = await readPrivateFileIfPresent(join(this.#root, "scheduler", name), 1_024 * 1_024);

        // continue to the older slot only when the newer slot is absent
        if (bytes === null) {
          continue;
        }
        return validateSchedulerState(JSON.parse(bytes.toString("utf8")));
      } catch {
        // try the previous complete slot after one invalid current slot
        invalidObserved = true;
      }
    }

    // never reset a known corrupt durable history to an empty state
    if (invalidObserved) {
      throw new Error("adjustment evidence scheduler state is invalid");
    }
    return createSchedulerState();
  }

  // rotate current to previous and fsync one bounded replacement
  async write(state) {
    validateSchedulerState(state);
    const directory = join(this.#root, "scheduler");
    const currentPath = join(directory, "current.json");
    const previousPath = join(directory, "previous.json");
    const current = await readPrivateFileIfPresent(currentPath, 1_024 * 1_024);

    // preserve the last complete generation before replacement
    if (current !== null) {
      try {
        validateSchedulerState(JSON.parse(current.toString("utf8")));
        await writeAtomicPrivate(previousPath, current);
      } catch {
        // preserve the existing previous slot instead of rotating corruption
      }
    }
    await writeAtomicPrivate(currentPath, Buffer.from(`${canonicalJson(state)}\n`));
  }
}

// schedule traffic-independent trusted forecast captures
export class AdjustmentEvidenceScheduler {
  #admission = { active: false, reason: "not_initialized" };
  #options;
  #running = false;
  #state = createSchedulerState();
  #timer = null;

  // retain injectable clocks, timers, trigger and filesystem probes
  constructor(options = {}) {
    this.#options = {
      clearInterval: options.clearInterval ?? clearInterval,
      enabled: options.enabled === true,
      migrationReady: options.migrationReady === true,
      now: options.now ?? (() => new Date()),
      readScheduleSlots: options.readScheduleSlots ?? (async () => {
        throw new Error("adjustment evidence schedule reader is unavailable");
      }),
      root: resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT),
      setInterval: options.setInterval ?? setInterval,
      stateStore: options.stateStore ?? new AdjustmentEvidenceSchedulerStateStore({
        root: options.root,
      }),
      statfs: options.statfs ?? ((path) => statfs(path, { bigint: true })),
      trigger: options.trigger ?? (async () => {
        throw new Error("scheduler trigger is unavailable");
      }),
    };
  }

  // recover durable status and evaluate every activation gate
  async initialize() {
    try {
      await this.#options.stateStore.initialize();
      this.#state = await this.#options.stateStore.read();
      await this.#refreshAdmission(this.#options.now());
      this.#state = {
        ...this.#state,
        activation: this.#admission.reason,
        updatedAt: this.#options.now().toISOString(),
      };
      await this.#options.stateStore.write(this.#state);
    } catch {
      // keep normal forecast serving available when scheduler state refuses
      this.#admission = { active: false, reason: "state_unavailable" };
      this.#state = {
        ...createSchedulerState(),
        activation: "state_unavailable",
        updatedAt: this.#options.now().toISOString(),
      };
    }
    return this;
  }

  // expose one immutable bounded health projection
  status() {
    return Object.freeze({
      ...this.#state,
      attempts: Object.freeze(this.#state.attempts.map((entry) => Object.freeze({ ...entry }))),
      errors: Object.freeze(this.#state.errors.map((entry) => Object.freeze({ ...entry }))),
      gaps: Object.freeze(this.#state.gaps.map((entry) => Object.freeze({ ...entry }))),
    });
  }

  // run at most one current or catch-up due key
  async runDue() {
    // serialize timer and startup ticks without creating a request backlog
    if (this.#running) {
      return { status: "already_running" };
    }
    this.#running = true;

    try {
      const now = this.#options.now();
      const nowIso = now.toISOString();
      await this.#refreshAdmission(now);

      // remain truthfully inactive until all controller gates pass
      if (!this.#admission.active) {
        // retain an authenticated terminal horizon before stopping the timer
        if (this.#admission.reason === "capture_ended" &&
          this.#state.activation !== "capture_ended") {
          await this.#persistStatus("capture_ended", nowIso);
          this.stop();
        }
        return { status: this.#admission.reason };
      }

      // refuse a wall-clock rollback instead of backdating a capture
      if (this.#state.lastCheckedAt !== null &&
        now.getTime() < Date.parse(this.#state.lastCheckedAt)) {
        return { status: "clock_rollback" };
      }

      const due = currentAdjustmentEvidenceDue(now, this.#admission.captureEndAt);

      // wait without traffic when the next jittered due is still future
      if (due === null) {
        return { status: "waiting" };
      }

      const priorAttempt = this.#state.attempts.find(
        // recognize one already terminal due key
        (entry) => entry.dueKey === due.dueKey,
      );

      // never issue the same durable due key twice after restart
      if (priorAttempt !== undefined) {
        return { status: "current_already_processed", dueKey: due.dueKey };
      }

      const gaps = missedAdjustmentEvidenceDueKeys(
        this.#state.lastCheckedAt,
        now,
        due.dueKey,
        this.#admission.captureEndAt,
      ).map(
        // record every skipped cycle categorically without backdating capture
        (dueKey) => ({ dueKey, occurredAt: nowIso, reason: "missed_capture" }),
      );
      this.#state = {
        ...this.#state,
        gaps: retainSchedulerHistory([...this.#state.gaps, ...gaps]),
      };

      try {
        const result = await this.#options.trigger({
          channel: "scheduler_request",
          commitAt: nowIso,
          dueKey: due.dueKey,
          path: "/api/v1/sites/ballydidean/forecast?days=10",
        });
        const attempt = {
          committedAt: nowIso,
          dueKey: due.dueKey,
          status: normalizeSchedulerTriggerStatus(result),
        };
        this.#state = {
          ...this.#state,
          activation: "active",
          attempts: retainSchedulerHistory([...this.#state.attempts, attempt]),
          lastCheckedAt: nowIso,
          updatedAt: nowIso,
        };
        await this.#options.stateStore.write(this.#state);
        return { ...attempt, scheduledAt: due.scheduledAt };
      } catch (error) {
        const failure = {
          at: nowIso,
          code: boundedErrorCode(error),
          dueKey: due.dueKey,
        };
        this.#state = {
          ...this.#state,
          attempts: retainSchedulerHistory([...this.#state.attempts, {
            committedAt: nowIso,
            dueKey: due.dueKey,
            status: "capture_failed",
          }]),
          errors: retainSchedulerHistory([...this.#state.errors, failure]),
          gaps: retainSchedulerHistory([...this.#state.gaps, {
            dueKey: due.dueKey,
            occurredAt: nowIso,
            reason: "capture_failed",
          }]),
          lastCheckedAt: nowIso,
          updatedAt: nowIso,
        };
        await this.#options.stateStore.write(this.#state);
        return { status: "capture_failed", dueKey: due.dueKey };
      }
    } finally {
      this.#running = false;
    }
  }

  // start one immediate restart reconciliation and periodic clock tick
  start() {
    // keep inactive units compiled but timer-free
    if (!this.#admission.active || this.#timer !== null) {
      return false;
    }

    void this.runDue().catch(
      // contain scheduler persistence faults outside request serving
      () => undefined,
    );
    this.#timer = this.#options.setInterval(
      // poll the clock independently of public traffic
      () => void this.runDue().catch(
        // contain later scheduler persistence faults outside request serving
        () => undefined,
      ),
      SCHEDULER_POLL_MILLISECONDS,
    );
    this.#timer.unref?.();
    return true;
  }

  // stop only this process-local clock timer
  stop() {
    // clear only an installed scheduler timer
    if (this.#timer !== null) {
      this.#options.clearInterval(this.#timer);
      this.#timer = null;
    }
  }

  // refresh the authenticated append-only schedule head before every due decision
  async #refreshAdmission(now) {
    let scheduleSlots = null;

    // avoid remote schedule reads while the controller or migration is inert
    if (this.#options.enabled && this.#options.migrationReady) {
      try {
        scheduleSlots = await this.#options.readScheduleSlots();
      } catch {
        scheduleSlots = null;
      }
    }
    this.#admission = await evaluateAdjustmentEvidenceSchedulerAdmission({
      enabled: this.#options.enabled,
      migrationReady: this.#options.migrationReady,
      now,
      root: this.#options.root,
      scheduleSlots,
      statfs: this.#options.statfs,
    });
  }

  // persist one non-capture scheduler status transition
  async #persistStatus(activation, updatedAt) {
    this.#state = {
      ...this.#state,
      activation: activation === "capture_ended" ? activation : this.#admission.reason,
      lastCheckedAt: updatedAt,
      updatedAt,
    };
    await this.#options.stateStore.write(this.#state);
  }
}

// move acknowledged scheduler evidence through two bounded online slots
export class AdjustmentEvidenceArchiveTransport {
  #now;
  #ports;
  #root;

  // retain only injectable clocks and the existing private evidence root
  constructor(options = {}) {
    this.#now = options.now ?? (() => new Date());
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#ports = options.ports ?? new AdjustmentCyclePageDiskPorts({
      now: this.#now,
      root: this.#root,
      statfs: options.statfs,
    });
  }

  // initialize the fixed online page directories without activating capture
  async initialize() {
    await this.#ports.initialize();
    return this;
  }

  // append one already committed trusted scheduler projection without blocking serving
  async captureCommittedSchedulerEvidence(input) {
    requireExactKeys(input, ["dueKey", "prepared"], "archive capture input");
    requireSchedulerDueKey(input.dueKey);
    const identitySha256 = requireSha256(
      input.prepared?.edgeReceiptIdentitySha256,
      "prepared.edgeReceiptIdentitySha256",
    );

    // refuse any caller-selected or legacy channel at the archive boundary
    if (input.prepared?.issuanceChannel !== "scheduler_request") {
      throw new TypeError("archive capture issuance channel is invalid");
    }

    try {
      return await withArchiveTransportLock(this.#root, this.#now, async () => {
        const payload = await readCommittedSchedulerPayload(this.#root, input.prepared);
        const current = await this.#ports.readSlot("current");

        // never reuse one pending slot for a later due cycle
        if (current !== null && current.header.dueKey !== input.dueKey) {
          return await this.#recordGap(input.dueKey, identitySha256, "slot_unavailable");
        }

        // make an exact post-crash retry a no-op instead of a second projection
        if (current !== null && current.projections.some(
          // match the stable committed receipt identity
          (projection) => projection.identitySha256 === identitySha256,
        )) {
          // bind retry success to the exact original payload
          if (current.header.payloadSha256 !== sha256(payload)) {
            return await this.#recordGap(input.dueKey, identitySha256, "archive_refused");
          }
          return { servingBlocked: false, status: "page_pending_acknowledgement" };
        }

        const priorTransfers = (await this.#ports.listTransferAcknowledgements()).filter(
          // retain only this due key's immutable original generation
          (transfer) => transfer.header.dueKey === input.dueKey,
        );
        const generation = current?.header.generation ?? priorTransfers[0]?.header.generation ??
          String(Date.parse(input.dueKey.slice(8)));
        // an exact checkpointed retry must not create a gap or relabel first issuance
        const prior = priorTransfers.find(
          (transfer) => transfer.projections.some(
            (projection) => projection.identitySha256 === identitySha256,
          ),
        );
        if (prior !== undefined) {
          if (prior.header.payloadSha256 !== sha256(payload)) {
            return await this.#recordGap(input.dueKey, identitySha256, "archive_refused");
          }
          return { servingBlocked: false, status: "page_checkpointed" };
        }
        const sealed = await this.#ports.readInputSeal(input.dueKey, generation);
        // permanently refuse new inputs after explicit producer closure
        if (sealed !== null) {
          return await this.#recordGap(input.dueKey, identitySha256, "archive_refused");
        }
        const finalized = await this.#ports.findFinalManifest(input.dueKey, generation);

        // never backfill or reopen an already finalized causal cycle
        if (finalized !== null) {
          return await this.#recordGap(input.dueKey, identitySha256, "archive_refused");
        }

        const state = await this.#recoverOpenState(input.dueKey, current);
        return await appendCyclePageFailOpen(state, {
          payload,
          projections: [{ channel: "scheduler_request", identitySha256 }],
        }, this.#ports);
      });
    } catch {
      // retain an honest permanent gap while normal forecasts remain successful
      return await this.#recordGap(input.dueKey, identitySha256, "archive_refused");
    }
  }

  // close only one complete acknowledged input population after its producers finish
  async sealCycleInputs(input) {
    requireExactKeys(input, ["dueKey", "generation", "requiredInputs"], "cycle input closure");
    requireSchedulerDueKey(input.dueKey);
    return await withArchiveTransportLock(this.#root, this.#now, async () => {
      const current = await this.#ports.readSlot("current");
      const next = await this.#ports.readSlot("next");
      // never close over a page whose payload is not checkpointed
      if ([current, next].some((page) => page !== null &&
        page.header.dueKey === input.dueKey && page.header.generation === input.generation)) {
        throw new Error("cycle inputs are not fully acknowledged");
      }
      const transfers = (await this.#ports.listTransferAcknowledgements()).filter(
        // select only the exact original cycle generation
        (entry) => entry.header.dueKey === input.dueKey &&
          entry.header.generation === input.generation,
      ).sort(
        // recover the complete ordered checkpoint chain
        (left, right) => left.header.pageIndex - right.header.pageIndex,
      );
      if (transfers.length === 0) {
        throw new Error("cycle input closure has no acknowledged pages");
      }
      const pages = [];
      const acknowledgementHashes = [];
      // bind exact page-protocol durability as well as workstation checkpoints
      for (const [index, transfer] of transfers.entries()) {
        const acknowledgement = await this.#ports.readPageAcknowledgement(transfer.pageSha256);
        if (transfer.header.pageIndex !== index || acknowledgement === null) {
          throw new Error("cycle input closure has incomplete acknowledgement chain");
        }
        pages.push({
          pageIndex: index,
          pageSha256: transfer.pageSha256,
          predecessorPageSha256: transfer.header.predecessorPageSha256,
          payloadSha256: transfer.header.payloadSha256,
          payloadOffset: transfer.header.payloadOffset,
          payloadLength: transfer.payloadLength,
        });
        acknowledgementHashes.push(acknowledgement.acknowledgementSha256);
      }
      const identities = transfers.flatMap(
        // retain exactly the admitted projection set and no later input
        (transfer) => transfer.projections.map((projection) => projection.identitySha256),
      ).sort();
      const gaps = await this.#ports.readEvidenceGaps(input.dueKey, input.generation);
      const existing = await this.#ports.readInputSeal(input.dueKey, input.generation);
      const seal = {
        acknowledgedProjectionCount: identities.length,
        acknowledgedProjectionRootSha256: sha256(Buffer.from(`${canonicalJson(identities)}\n`)),
        acknowledgementRootSha256: sha256(Buffer.from(`${canonicalJson(acknowledgementHashes)}\n`)),
        contractVersion: "adjustment-cycle-input-seal/v1",
        dueKey: input.dueKey,
        evidenceGapRootSha256: sha256(Buffer.from(`${canonicalJson(gaps.map(
          // preserve every permanent gap in the same generation
          (gap) => gap.gapSha256,
        ).sort())}\n`)),
        generation: input.generation,
        pageRootSha256: sha256(Buffer.from(`${canonicalJson(pages)}\n`)),
        requiredInputs: input.requiredInputs,
        sealedAt: existing?.sealedAt ?? this.#now().toISOString(),
      };
      validateCycleInputSeal(seal);
      await this.#ports.persistInputSeal(seal);
      await this.#recoverAcknowledgedFinals();
      return Object.freeze(seal);
    });
  }

  // return only the current validated page or one canonical idle marker
  async next() {
    return await withArchiveTransportLock(this.#root, this.#now, async () => {
      await this.#recoverAcknowledgedFinals();
      const pendingFinal = await this.#ports.nextPendingFinalManifest();

      // transfer one genuine immutable cycle closure before later online pages
      if (pendingFinal !== null) {
        return Object.freeze({
          contractVersion: ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION,
          kind: "final",
          manifest: pendingFinal.manifest,
          manifestSha256: pendingFinal.manifestSha256,
        });
      }

      const current = await this.#recoverCurrentSlot();

      // disclose no mutable state when no page awaits acknowledgement
      if (current === null) {
        return Object.freeze({
          contractVersion: ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION,
          kind: "idle",
        });
      }

      const envelope = {
        contractVersion: ADJUSTMENT_ARCHIVE_TRANSFER_CONTRACT_VERSION,
        kind: "page",
        header: current.header,
        pageSha256: current.pageSha256,
        projections: current.projections,
        payloadBase64: current.payload.toString("base64"),
      };
      const bytes = Buffer.from(`${canonicalJson(envelope)}\n`);

      // enforce the closed SSH response ceiling before returning any bytes
      if (bytes.byteLength > ARCHIVE_TRANSFER_MAXIMUM_BYTES) {
        throw new Error("archive transfer envelope is too large");
      }
      return Object.freeze(envelope);
    });
  }

  // bind one page to the workstation open-cycle checkpoint before reusing its slot
  async acknowledge(pageSha256, checkpointSha256) {
    requireSha256(pageSha256, "pageSha256");
    requireSha256(checkpointSha256, "checkpointSha256");

    return await withArchiveTransportLock(this.#root, this.#now, async () => {
      const existing = await this.#ports.readTransferAcknowledgement(pageSha256);
      let acknowledgement = existing;

      // preserve the first fsynced workstation checkpoint for this page
      if (existing !== null) {
        if (existing.checkpointSha256 !== checkpointSha256) {
          throw new Error("archive acknowledgement collision");
        }
      } else {
        const current = await this.#ports.readSlot("current");

        // accept a new acknowledgement only for the exact pending current page
        if (current === null || current.pageSha256 !== pageSha256) {
          throw new Error("archive acknowledgement page is not pending");
        }
        acknowledgement = {
          acknowledgedAt: this.#now().toISOString(),
          contractVersion: "adjustment-archive-transfer-ack/v1",
          checkpointSha256,
          header: current.header,
          pageSha256,
          payloadLength: current.payload.length,
          projections: current.projections,
        };
        await this.#ports.persistTransferAcknowledgement(acknowledgement);
      }

      const current = await this.#ports.readSlot("current");

      // finish a crash-interrupted slot release using the original server clock
      if (current !== null && current.pageSha256 === pageSha256) {
        const pageAcknowledgement = {
          acknowledgedAt: acknowledgement.acknowledgedAt,
          contractVersion: "adjustment-cycle-page-ack/v1",
          dueKey: current.header.dueKey,
          generation: current.header.generation,
          pageIndex: current.header.pageIndex,
          pageSha256,
        };
        await this.#ports.persistAcknowledgement({
          acknowledgement: pageAcknowledgement,
          acknowledgementSha256: sha256(
            Buffer.from(`${canonicalJson(pageAcknowledgement)}\n`),
          ),
          nextSlots: (await this.#ports.readSlot("next")) === null ? [] : [{}],
        });
      } else if (current === null) {
        // finish a crash-interrupted next-slot promotion before returning the retry
        await this.#recoverCurrentSlot();
      }
      await this.#recoverAcknowledgedFinals();
      return Object.freeze({ ...acknowledgement });
    });
  }

  // bind one genuine final manifest to its first fully verified cold graph
  async acknowledgeFinal(manifestSha256, graphSha256) {
    requireSha256(manifestSha256, "manifestSha256");
    requireSha256(graphSha256, "graphSha256");

    return await withArchiveTransportLock(this.#root, this.#now, async () => {
      const existing = await this.#ports.readFinalAcknowledgement(manifestSha256);

      // preserve the first final graph binding on exact retries
      if (existing !== null) {
        if (existing.graphSha256 !== graphSha256) {
          throw new Error("archive final acknowledgement collision");
        }
        return Object.freeze({ ...existing });
      }

      await this.#recoverAcknowledgedFinals();
      const final = await this.#ports.nextPendingFinalManifest();

      // accept T only for the exact oldest genuine pending final manifest
      if (final === null || final.manifestSha256 !== manifestSha256) {
        throw new Error("archive final manifest is not pending");
      }
      const acknowledgement = {
        acknowledgedAt: this.#now().toISOString(),
        contractVersion: "adjustment-archive-final-ack/v1",
        graphSha256,
        manifestSha256,
      };
      await this.#ports.persistFinalAcknowledgement(acknowledgement);
      return Object.freeze(acknowledgement);
    });
  }

  // recover the full acknowledged prefix and at most two retained payload slots
  async #recoverOpenState(dueKey, current) {
    const transfers = (await this.#ports.listTransferAcknowledgements()).filter(
      // retain original due identity rather than resetting an acknowledged prefix
      (transfer) => transfer.header.dueKey === dueKey,
    ).sort(
      // reconstruct the exact immutable page sequence
      (left, right) => left.header.pageIndex - right.header.pageIndex,
    );
    const generation = current?.header.generation ?? transfers[0]?.header.generation ??
      String(Date.parse(dueKey.slice(8)));
    const state = createCyclePageState({
      dailyPageCount: Number(dueKey.slice(19, 21)) / 6,
      dueKey,
      generation,
      localDate: dueKey.slice(8, 18),
    });
    const next = await this.#ports.readSlot("next");
    // never treat an orphan next payload as a fresh empty chain
    if (current === null && next !== null) {
      throw new Error("archive next slot has no current predecessor");
    }
    const durablePages = [];
    const acknowledgements = [];
    // restore every checkpointed metadata record without restoring its payload
    for (const transfer of transfers) {
      if (transfer.header.generation !== generation) {
        throw new Error("archive due generation collision");
      }
      const acknowledgement = await this.#ports.readPageAcknowledgement(transfer.pageSha256);
      if (acknowledgement === null) {
        throw new Error("archive checkpoint acknowledgement is incomplete");
      }
      durablePages.push({
        pageIndex: transfer.header.pageIndex,
        pageSha256: transfer.pageSha256,
        predecessorPageSha256: transfer.header.predecessorPageSha256,
        payloadSha256: transfer.header.payloadSha256,
        payloadOffset: transfer.header.payloadOffset,
        payloadLength: transfer.payloadLength,
        projectionIdentities: transfer.projections.map(
          (projection) => projection.identitySha256,
        ),
        projectionChannels: transfer.projections.map((projection) => projection.channel),
      });
      acknowledgements.push(acknowledgement);
    }
    // append only pending slots not already represented by a durable checkpoint
    for (const page of [current, next]) {
      if (page === null || durablePages.some((value) => value.pageSha256 === page.pageSha256)) {
        continue;
      }
      if (page.header.dueKey !== dueKey || page.header.generation !== generation) {
        throw new Error("archive pending slot identity collision");
      }
      durablePages.push({
        pageIndex: page.header.pageIndex,
        pageSha256: page.pageSha256,
        predecessorPageSha256: page.header.predecessorPageSha256,
        payloadSha256: page.header.payloadSha256,
        payloadOffset: page.header.payloadOffset,
        payloadLength: page.payload.length,
        projectionIdentities: page.projections.map((projection) => projection.identitySha256),
        projectionChannels: page.projections.map((projection) => projection.channel),
      });
    }
    const gaps = await this.#ports.readEvidenceGaps(dueKey, generation);
    return reconcileCyclePages({
      durable: { acknowledgements, finalized: null, gaps, pages: durablePages },
      expected: state,
    }).state;
  }

  // promote only a next page whose predecessor has an exact durable checkpoint
  async #recoverCurrentSlot() {
    const current = await this.#ports.readSlot("current");

    // return the ordinary complete state without filesystem mutation
    if (current !== null) {
      return current;
    }

    const next = await this.#ports.readSlot("next");

    // return the stable empty state when no crash promotion remains
    if (next === null) {
      return null;
    }

    const predecessor = next.header.predecessorPageSha256;

    // refuse an orphaned next slot without a durable predecessor acknowledgement
    if (predecessor === null ||
      await this.#ports.readTransferAcknowledgement(predecessor) === null) {
      throw new Error("archive next slot predecessor is not acknowledged");
    }
    await this.#ports.promoteNext();
    return await this.#ports.readSlot("current");
  }

  // finalize each crash-recoverable scheduler cycle after every page acknowledgement
  async #recoverAcknowledgedFinals() {
    const acknowledgements = await this.#ports.listTransferAcknowledgements();
    const groups = new Map();

    // group bounded checkpoint metadata by the exact due and generation identity
    for (const transfer of acknowledgements) {
      const key = `${transfer.header.dueKey}\u0000${transfer.header.generation}`;
      const group = groups.get(key) ?? [];
      group.push(transfer);
      groups.set(key, group);
    }

    const current = await this.#ports.readSlot("current");
    const next = await this.#ports.readSlot("next");

    // recover each finite scheduler-only cycle independently
    for (const transfers of groups.values()) {
      transfers.sort(
        // reconstruct exact page order from checkpointed indices
        (left, right) => left.header.pageIndex - right.header.pageIndex,
      );
      const first = transfers[0];

      const inputSeal = await this.#ports.readInputSeal(
        first.header.dueKey, first.header.generation,
      );
      // acknowledgements never imply that all body and revision producers finished
      if (inputSeal === null) {
        continue;
      }

      const existing = await this.#ports.findFinalManifest(
        first.header.dueKey,
        first.header.generation,
      );

      // retain the already immutable final manifest without rewriting it
      if (existing !== null) {
        continue;
      }

      const slotStillOpen = [current, next].some(
        // keep the cycle open until every physical page is acknowledged
        (page) => page !== null && page.header.dueKey === first.header.dueKey &&
          page.header.generation === first.header.generation,
      );

      // never close a cycle while current or next still retains one of its pages
      if (slotStillOpen) {
        continue;
      }

      const pageAcknowledgements = [];

      // recover every durable page acknowledgement in exact page order
      for (const transfer of transfers) {
        const pageAcknowledgement = await this.#ports.readPageAcknowledgement(
          transfer.pageSha256,
        );

        // wait until every page-protocol acknowledgement is durable after a crash
        if (pageAcknowledgement === null) {
          break;
        }
        pageAcknowledgements.push(pageAcknowledgement);
      }

      // refuse closure over a partial acknowledgement prefix
      if (pageAcknowledgements.length !== transfers.length) {
        continue;
      }

      const gaps = await this.#ports.readEvidenceGaps(
        first.header.dueKey,
        first.header.generation,
      );
      const pages = transfers.map(
        // restore exact bounded page metadata without retaining payload bytes
        (transfer) => ({
          pageIndex: transfer.header.pageIndex,
          pageSha256: transfer.pageSha256,
          predecessorPageSha256: transfer.header.predecessorPageSha256,
          payloadSha256: transfer.header.payloadSha256,
          payloadOffset: transfer.header.payloadOffset,
          payloadLength: transfer.payloadLength,
          projectionIdentities: transfer.projections.map(
            // retain exact checkpointed projection identities
            (projection) => projection.identitySha256,
          ),
          projectionChannels: transfer.projections.map(
            // retain exact checkpointed trusted channels
            (projection) => projection.channel,
          ),
        }),
      );
      const state = {
        contractVersion: "adjustment-cycle-page-state/v1",
        dueKey: first.header.dueKey,
        generation: first.header.generation,
        localDate: first.header.dueKey.slice(8, 18),
        dailyPageCountAtOpen: Number(first.header.dueKey.slice(19, 21)) / 6,
        pages,
        acknowledgements: pageAcknowledgements,
        gaps,
        slots: [],
        organicProjectionCount: transfers.reduce(
          // preserve the combined cycle's non-scheduler population
          (total, transfer) => total + transfer.projections.filter(
            (projection) => projection.channel === "organic",
          ).length,
          0,
        ),
        schedulerProjectionCount: transfers.reduce(
          // count every checkpointed scheduler projection exactly once
          (total, transfer) => total + transfer.projections.filter(
            (projection) => projection.channel === "scheduler_request",
          ).length,
          0,
        ),
        finalized: null,
      };
      await finalizeCyclePages(state, {
        finalizedAt: inputSeal.sealedAt,
        inputSeal,
      }, this.#ports);
    }
  }

  // fsync one forever-unqualified projection without delaying serving
  async #recordGap(dueKey, identitySha256, reason) {
    try {
      // reuse the first categorical gap without relabelling its occurrence clock
      if (await this.#ports.hasEvidenceGap(dueKey, identitySha256)) {
        return { servingBlocked: false, status: "evidence_gap" };
      }
      const state = createCyclePageState({
        dailyPageCount: Number(dueKey.slice(19, 21)) / 6,
        dueKey,
        generation: String(Date.parse(dueKey.slice(8))),
        localDate: dueKey.slice(8, 18),
      });
      await recordEvidenceGap(state, {
        occurredAt: this.#now().toISOString(),
        projectionIdentitySha256: identitySha256,
        reason,
      }, this.#ports);
      return { servingBlocked: false, status: "evidence_gap" };
    } catch {
      return {
        reason,
        servingBlocked: false,
        status: "evidence_gap_persistence_refused",
      };
    }
  }
}

// persist current and next online page slots below the evidence root
export class AdjustmentCyclePageDiskPorts {
  #now;
  #root;
  #statfs;

  // retain one existing evidence-volume root without path overrides
  constructor(options = {}) {
    this.#now = options.now ?? (() => new Date());
    this.#root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
    this.#statfs = options.statfs ?? ((path) => statfs(path, { bigint: true }));
  }

  // provide one injected clock for fail-open gap evidence
  now() {
    return this.#now();
  }

  // create fixed private page-state directories only
  async initialize() {
    const root = join(this.#root, "online-pages");
    await requirePrivateDirectory(this.#root);
    await ensurePrivateDirectory(root);

    // create every bounded metadata class beneath the fixed root
    for (const name of [
      "acknowledgements",
      "archive-acknowledgements",
      "final-acknowledgements",
      "final-manifests",
      "gaps",
      "input-seals",
      "locks",
      "slots",
    ]) {
      await ensurePrivateDirectory(join(root, name));
    }
    return this;
  }

  // read one bounded slot without following links or accepting noncanonical metadata
  async readSlot(slotValue) {
    const slot = slotValue === "current" ? "current" : slotValue === "next" ? "next" : null;

    // reject caller-named slot paths
    if (slot === null) {
      throw new TypeError("online page slot is invalid");
    }

    let bytes;

    try {
      bytes = await readRegularFile(
        join(this.#root, "online-pages", "slots", `${slot}.page`),
        320 * 1_024,
      );
    } catch (error) {
      // return only an actually absent slot as empty
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }

    // reject short prefixes and unreasonable metadata lengths before parsing
    if (bytes.byteLength < 5) {
      throw new Error("online page slot is truncated");
    }
    const metadataLength = bytes.readUInt32BE(0);
    const metadataEnd = 4 + metadataLength;

    // bind metadata and payload within the fixed page ceiling
    if (metadataLength < 2 || metadataLength > 32 * 1_024 || metadataEnd >= bytes.byteLength) {
      throw new Error("online page slot metadata is invalid");
    }
    const metadataBytes = bytes.subarray(4, metadataEnd);
    const metadata = JSON.parse(metadataBytes.toString("utf8"));

    // require the exact canonical metadata encoding written by persistPage
    if (!metadataBytes.equals(Buffer.from(`${canonicalJson(metadata)}\n`))) {
      throw new Error("online page slot metadata is not canonical");
    }
    return validateCyclePage({
      header: metadata.header,
      pageSha256: metadata.pageSha256,
      payload: bytes.subarray(metadataEnd),
      projections: metadata.projections,
    });
  }

  // fsync one exclusive current or next page slot
  async persistPage(input) {
    const slot = input.slot === "current" ? "current" : input.slot === "next" ? "next" : null;

    // refuse a third or caller-named slot
    if (slot === null) {
      throw new TypeError("online page slot is invalid");
    }
    const metadata = Buffer.from(`${canonicalJson({
      header: input.header,
      pageSha256: input.pageSha256,
      projections: input.projections,
    })}\n`);
    const prefix = Buffer.alloc(4);
    prefix.writeUInt32BE(metadata.byteLength);
    const bytes = Buffer.concat([prefix, metadata, Buffer.from(input.payload)]);
    await this.#admit(bytes.byteLength, 1);
    await writeOrVerifyImmutable(
      join(this.#root, "online-pages", "slots", `${slot}.page`),
      bytes,
      320 * 1_024,
      writeExclusive,
    );
    return { fsynced: true, pageSha256: input.pageSha256 };
  }

  // read one first-writer checkpoint acknowledgement by exact page identity
  async readTransferAcknowledgement(pageSha256) {
    requireSha256(pageSha256, "pageSha256");
    const path = join(
      this.#root,
      "online-pages",
      "archive-acknowledgements",
      `sha256-${pageSha256}.json`,
    );
    let bytes;

    try {
      bytes = await readRegularFile(path, 2_048);
    } catch (error) {
      // return only a genuinely absent acknowledgement as missing
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    const value = JSON.parse(bytes.toString("utf8"));
    requireExactKeys(value, [
      "acknowledgedAt",
      "checkpointSha256",
      "contractVersion",
      "header",
      "pageSha256",
      "payloadLength",
      "projections",
    ], "archive transfer acknowledgement");
    requireEqual(
      value.contractVersion,
      "adjustment-archive-transfer-ack/v1",
      "archive transfer acknowledgement contractVersion",
    );
    requireEqual(value.pageSha256, pageSha256, "archive transfer acknowledgement pageSha256");
    requireSha256(
      value.checkpointSha256,
      "archive transfer acknowledgement checkpointSha256",
    );
    requireInstant(value.acknowledgedAt, "archive transfer acknowledgement acknowledgedAt");
    validateTransferCheckpointMetadata(value);

    // require exact canonical bytes for every idempotent retry
    if (!bytes.equals(Buffer.from(`${canonicalJson(value)}\n`))) {
      throw new Error("archive transfer acknowledgement is not canonical");
    }
    return value;
  }

  // fsync the first workstation open-cycle checkpoint before releasing its page slot
  async persistTransferAcknowledgement(acknowledgement) {
    requireExactKeys(acknowledgement, [
      "acknowledgedAt",
      "checkpointSha256",
      "contractVersion",
      "header",
      "pageSha256",
      "payloadLength",
      "projections",
    ], "archive transfer acknowledgement");
    requireEqual(
      acknowledgement.contractVersion,
      "adjustment-archive-transfer-ack/v1",
      "archive transfer acknowledgement contractVersion",
    );
    validateTransferCheckpointMetadata(acknowledgement);
    requireSha256(
      acknowledgement.checkpointSha256,
      "archive transfer acknowledgement checkpointSha256",
    );
    requireInstant(
      acknowledgement.acknowledgedAt,
      "archive transfer acknowledgement acknowledgedAt",
    );
    const bytes = Buffer.from(`${canonicalJson(acknowledgement)}\n`);
    await this.#admit(bytes.byteLength, 1);
    const path = join(
      this.#root,
      "online-pages",
      "archive-acknowledgements",
      `sha256-${acknowledgement.pageSha256}.json`,
    );
    await writeOrVerifyImmutable(path, bytes, 2_048, writeExclusive);
    return { fsynced: true, pageSha256: acknowledgement.pageSha256 };
  }

  // list the finite validated page checkpoints for crash reconciliation
  async listTransferAcknowledgements() {
    const directory = join(this.#root, "online-pages", "archive-acknowledgements");
    const names = (await readdir(directory)).sort();

    // refuse an unexpected lifetime checkpoint spool
    if (names.length > 8_192) {
      throw new Error("archive checkpoint history exceeds its bound");
    }

    const acknowledgements = [];

    // validate every content-addressed checkpoint before reconstruction
    for (const name of names) {
      const match = /^sha256-([a-f0-9]{64})\.json$/u.exec(name);

      // reject foreign files in the fixed owner-private directory
      if (match === null) {
        throw new Error("archive checkpoint filename is invalid");
      }
      acknowledgements.push(await this.readTransferAcknowledgement(match[1]));
    }
    return acknowledgements;
  }

  // find the durable page-protocol acknowledgement for one transferred page
  async readPageAcknowledgement(pageSha256) {
    requireSha256(pageSha256, "pageSha256");
    const directory = join(this.#root, "online-pages", "acknowledgements");
    const names = (await readdir(directory)).sort();

    // retain a finite restart scan for the fixed capture horizon
    if (names.length > 8_192) {
      throw new Error("page acknowledgement history exceeds its bound");
    }

    // inspect each canonical acknowledgement until its page identity matches
    for (const name of names) {
      const match = /^sha256-([a-f0-9]{64})\.json$/u.exec(name);

      // reject foreign files in the fixed metadata directory
      if (match === null) {
        throw new Error("page acknowledgement filename is invalid");
      }
      const bytes = await readRegularFile(join(directory, name), 2_048);
      const value = JSON.parse(bytes.toString("utf8"));

      // bind exact canonical acknowledgement bytes to their filename
      if (!bytes.equals(Buffer.from(`${canonicalJson(value)}\n`)) ||
        sha256(bytes) !== match[1]) {
        throw new Error("page acknowledgement is invalid");
      }
      requireExactKeys(value, [
        "acknowledgedAt",
        "contractVersion",
        "dueKey",
        "generation",
        "pageIndex",
        "pageSha256",
      ], "page acknowledgement");
      requireEqual(
        value.contractVersion,
        ADJUSTMENT_CYCLE_ACK_CONTRACT_VERSION,
        "page acknowledgement contractVersion",
      );
      requireSchedulerDueKey(value.dueKey);
      requireSha256(value.pageSha256, "page acknowledgement pageSha256");
      requireInstant(value.acknowledgedAt, "page acknowledgement acknowledgedAt");

      // retain exact unsigned generation and bounded page index fields
      if (!/^(?:0|[1-9][0-9]*)$/u.test(value.generation) ||
        BigInt(value.generation) > 0xffff_ffff_ffff_ffffn ||
        !Number.isSafeInteger(value.pageIndex) || value.pageIndex < 0 ||
        value.pageIndex >= ADJUSTMENT_CYCLE_MAXIMUM_PAGES) {
        throw new Error("page acknowledgement fields are invalid");
      }

      // return only the requested page's closed acknowledgement schema
      if (value.pageSha256 === pageSha256) {
        return { ...value, acknowledgementSha256: match[1] };
      }
    }
    return null;
  }

  // fsync acknowledgement before releasing and rotating one page slot
  async persistAcknowledgement(input) {
    const bytes = Buffer.from(`${canonicalJson(input.acknowledgement)}\n`);
    await this.#admit(bytes.byteLength, 1);
    await writeOrVerifyImmutable(
      join(this.#root, "online-pages", "acknowledgements", `sha256-${input.acknowledgementSha256}.json`),
      bytes,
      2_048,
      writeExclusive,
    );
    const slots = join(this.#root, "online-pages", "slots");
    const current = await this.readSlot("current");

    // unlink only the exact page bound by the durable acknowledgement
    if (current !== null && current.pageSha256 === input.acknowledgement.pageSha256) {
      await unlink(join(slots, "current.page"));
    } else if (current !== null && current.header.predecessorPageSha256 !==
      input.acknowledgement.pageSha256) {
      throw new Error("online page acknowledgement slot collision");
    }

    // promote only an already durable next slot
    if (input.nextSlots.length === 1 && await this.readSlot("current") === null) {
      try {
        await rename(join(slots, "next.page"), join(slots, "current.page"));
      } catch (error) {
        // accept only a retry after the next slot was already promoted
        if (error?.code !== "ENOENT" || await this.readSlot("current") === null) {
          throw error;
        }
      }
    }
    await fsyncDirectory(slots);
    return { acknowledgementSha256: input.acknowledgementSha256, fsynced: true };
  }

  // promote one crash-left next slot after its predecessor acknowledgement is proven
  async promoteNext() {
    const slots = join(this.#root, "online-pages", "slots");
    const current = await this.readSlot("current");

    // avoid replacing any already published current page
    if (current !== null) {
      throw new Error("online current page already exists");
    }
    await rename(join(slots, "next.page"), join(slots, "current.page"));
    await fsyncDirectory(slots);
  }

  // fsync one immutable categorical gap
  async persistGap(input) {
    const bytes = Buffer.from(`${canonicalJson(input.gap)}\n`);
    await this.#admit(bytes.byteLength, 1);
    await writeExclusive(
      join(this.#root, "online-pages", "gaps", `sha256-${input.gapSha256}.json`),
      bytes,
    );
    return { fsynced: true, gapSha256: input.gapSha256 };
  }

  // find one prior immutable gap without creating a second occurrence for retries
  async hasEvidenceGap(dueKey, projectionIdentitySha256) {
    requireSchedulerDueKey(dueKey);
    requireSha256(projectionIdentitySha256, "projectionIdentitySha256");
    return (await this.#listEvidenceGaps()).some(
      // reuse only the first exact due and stable projection identity
      (value) => value.dueKey === dueKey &&
        value.projectionIdentitySha256 === projectionIdentitySha256,
    );
  }

  // return every durable permanent gap belonging to one exact cycle
  async readEvidenceGaps(dueKey, generation) {
    requireSchedulerDueKey(dueKey);
    return (await this.#listEvidenceGaps()).filter(
      // retain only gaps bound to the finalizing cycle identity
      (value) => value.dueKey === dueKey && value.generation === generation,
    );
  }

  // read only one exact canonical combined-input seal
  async readInputSeal(dueKey, generation) {
    requireSchedulerDueKey(dueKey);
    const identity = sha256(Buffer.from(`${canonicalJson({ dueKey, generation })}\n`));
    let bytes;
    try {
      bytes = await readRegularFile(
        join(this.#root, "online-pages", "input-seals", `sha256-${identity}.json`), 2_048,
      );
    } catch (error) {
      // return absence without treating malformed state as an empty generation
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    const seal = validateCycleInputSeal(JSON.parse(bytes.toString("utf8")));
    if (seal.dueKey !== dueKey || seal.generation !== generation ||
      !bytes.equals(Buffer.from(`${canonicalJson(seal)}\n`))) {
      throw new Error("cycle input seal identity is invalid");
    }
    return seal;
  }

  // publish one immutable producer closure without resetting a prior generation
  async persistInputSeal(seal) {
    validateCycleInputSeal(seal);
    const bytes = Buffer.from(`${canonicalJson(seal)}\n`);
    const identity = sha256(Buffer.from(`${canonicalJson({
      dueKey: seal.dueKey, generation: seal.generation,
    })}\n`));
    const path = join(this.#root, "online-pages", "input-seals", `sha256-${identity}.json`);
    const existing = await this.readInputSeal(seal.dueKey, seal.generation);
    // charge allocation only when no identical seal already exists
    if (existing === null) {
      await this.#admit(bytes.length, 1);
    }
    await writeOrVerifyImmutable(path, bytes, 2_048, writeExclusive);
    return { fsynced: true };
  }

  // fsync one immutable final cycle manifest
  async persistFinalManifest(input) {
    const bytes = Buffer.from(`${canonicalJson(input.manifest)}\n`);
    await this.#admit(bytes.byteLength, 1);
    await writeOrVerifyImmutable(
      join(this.#root, "online-pages", "final-manifests", `sha256-${input.manifestSha256}.json`),
      bytes,
      32 * 1_024,
      writeExclusive,
    );
    return { fsynced: true, manifestSha256: input.manifestSha256 };
  }

  // read one exact immutable final manifest by content identity
  async readFinalManifest(manifestSha256) {
    requireSha256(manifestSha256, "manifestSha256");
    let bytes;

    try {
      bytes = await readRegularFile(
        join(
          this.#root,
          "online-pages",
          "final-manifests",
          `sha256-${manifestSha256}.json`,
        ),
        32 * 1_024,
      );
    } catch (error) {
      // return only an actually absent final manifest as missing
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    const manifest = JSON.parse(bytes.toString("utf8"));
    validateCycleFinalManifest(manifest);

    // bind exact canonical manifest bytes to the requested identity
    if (!bytes.equals(Buffer.from(`${canonicalJson(manifest)}\n`)) ||
      sha256(bytes) !== manifestSha256) {
      throw new Error("archive final manifest is invalid");
    }
    return { manifest, manifestSha256 };
  }

  // find one immutable closure for the exact due and generation pair
  async findFinalManifest(dueKey, generation) {
    const manifests = await this.#listFinalManifests();
    return manifests.find(
      // match the exact online cycle identity
      (entry) => entry.manifest.dueKey === dueKey &&
        entry.manifest.generation === generation,
    ) ?? null;
  }

  // return the oldest finalized cycle without a cold-graph acknowledgement
  async nextPendingFinalManifest() {
    const manifests = await this.#listFinalManifests();
    manifests.sort(
      // retain deterministic cycle order across restarts
      (left, right) => left.manifest.dueKey.localeCompare(right.manifest.dueKey, "en"),
    );

    // choose only the first final without a T receipt
    for (const manifest of manifests) {
      if (await this.readFinalAcknowledgement(manifest.manifestSha256) === null) {
        return manifest;
      }
    }
    return null;
  }

  // read one immutable final cold-graph receipt
  async readFinalAcknowledgement(manifestSha256) {
    requireSha256(manifestSha256, "manifestSha256");
    let bytes;

    try {
      bytes = await readRegularFile(
        join(
          this.#root,
          "online-pages",
          "final-acknowledgements",
          `sha256-${manifestSha256}.json`,
        ),
        2_048,
      );
    } catch (error) {
      // return only an absent final acknowledgement as missing
      if (error?.code === "ENOENT") {
        return null;
      }
      throw error;
    }
    const value = JSON.parse(bytes.toString("utf8"));
    validateFinalTransferAcknowledgement(value, manifestSha256);

    // require canonical first-writer bytes on every retry
    if (!bytes.equals(Buffer.from(`${canonicalJson(value)}\n`))) {
      throw new Error("archive final acknowledgement is not canonical");
    }
    return value;
  }

  // fsync the first full cold-graph binding for one final manifest
  async persistFinalAcknowledgement(acknowledgement) {
    validateFinalTransferAcknowledgement(
      acknowledgement,
      acknowledgement.manifestSha256,
    );
    const bytes = Buffer.from(`${canonicalJson(acknowledgement)}\n`);
    await this.#admit(bytes.byteLength, 1);
    await writeOrVerifyImmutable(
      join(
        this.#root,
        "online-pages",
        "final-acknowledgements",
        `sha256-${acknowledgement.manifestSha256}.json`,
      ),
      bytes,
      2_048,
      writeExclusive,
    );
    return { fsynced: true, manifestSha256: acknowledgement.manifestSha256 };
  }

  // list each finite validated final manifest for transfer ordering
  async #listFinalManifests() {
    const directory = join(this.#root, "online-pages", "final-manifests");
    const names = (await readdir(directory)).sort();

    // refuse an unexpected lifetime manifest spool
    if (names.length > 8_192) {
      throw new Error("archive final manifest history exceeds its bound");
    }

    const manifests = [];

    // validate every content-addressed final before returning it
    for (const name of names) {
      const match = /^sha256-([a-f0-9]{64})\.json$/u.exec(name);

      // reject foreign files in the fixed final directory
      if (match === null) {
        throw new Error("archive final manifest filename is invalid");
      }
      manifests.push(await this.readFinalManifest(match[1]));
    }
    return manifests;
  }

  // list each bounded canonical permanent gap for retry and finalization
  async #listEvidenceGaps() {
    const directory = join(this.#root, "online-pages", "gaps");
    const names = (await readdir(directory)).sort();

    // refuse an unexpected unbounded gap directory instead of scanning forever
    if (names.length > 8_192) {
      throw new Error("online page gap history exceeds its bound");
    }

    const gaps = [];

    // inspect every bounded canonical gap in content-address order
    for (const name of names) {
      const match = /^sha256-([a-f0-9]{64})\.json$/u.exec(name);

      // reject foreign files in the owner-private fixed directory
      if (match === null) {
        throw new Error("online page gap filename is invalid");
      }
      const bytes = await readRegularFile(join(directory, name), 2_048);
      const value = JSON.parse(bytes.toString("utf8"));
      requireExactKeys(value, [
        "contractVersion",
        "dueKey",
        "generation",
        "projectionIdentitySha256",
        "reason",
        "occurredAt",
        "qualificationDisposition",
      ], "online page gap");
      requireEqual(
        value.contractVersion,
        ADJUSTMENT_EVIDENCE_GAP_CONTRACT_VERSION,
        "online page gap contractVersion",
      );
      requireSchedulerDueKey(value.dueKey);
      requireSha256(value.projectionIdentitySha256, "online page gap projection identity");
      requireInstant(value.occurredAt, "online page gap occurredAt");

      // retain only the page protocol's closed permanent-gap categories
      if (![
        "archive_refused",
        "capacity_refused",
        "page_limit",
        "projection_limit",
        "slot_unavailable",
        "transport_refused",
        "unacknowledged",
      ].includes(value.reason) || value.qualificationDisposition !== "forever_unqualified" ||
        !/^(?:0|[1-9][0-9]*)$/u.test(value.generation)) {
        throw new Error("online page gap fields are invalid");
      }

      // bind canonical bytes and the content-addressed filename
      if (!bytes.equals(Buffer.from(`${canonicalJson(value)}\n`)) ||
        sha256(bytes) !== match[1]) {
        throw new Error("online page gap is invalid");
      }
      gaps.push({ ...value, gapSha256: match[1] });
    }
    return gaps;
  }

  // preserve the protected floor, capture reservation and inodes before each write
  async #admit(byteLength, files) {
    const filesystem = await this.#statfs(this.#root);
    const blockSize = Number(filesystem.bsize);
    const freeInodes = Number(filesystem.ffree);
    const prospectiveBytes = allocatedSize(byteLength, blockSize);
    const freeBytes = Number(filesystem.bavail * filesystem.bsize);

    // refuse without deleting or reusing any acknowledged history
    if (!Number.isSafeInteger(blockSize) || blockSize < 1 ||
      !Number.isSafeInteger(freeBytes) || !Number.isSafeInteger(freeInodes) ||
      freeBytes - prospectiveBytes < ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES ||
      freeInodes - files < FREE_INODE_FLOOR) {
      const error = new Error("online page capacity is exhausted");
      error.code = "resource_refused";
      throw error;
    }
  }
}

// validate one deleted-payload checkpoint without fabricating page bytes
function validateTransferCheckpointMetadata(value) {
  requireSha256(value.pageSha256, "archive transfer acknowledgement pageSha256");
  const header = value.header;
  requireExactKeys(header, [
    "contractVersion",
    "dueKey",
    "generation",
    "pageIndex",
    "predecessorPageSha256",
    "payloadSha256",
    "payloadOffset",
  ], "archive transfer acknowledgement header");
  requireEqual(
    header.contractVersion,
    ADJUSTMENT_CYCLE_PAGE_CONTRACT_VERSION,
    "archive transfer acknowledgement page contractVersion",
  );
  requireSchedulerDueKey(header.dueKey);
  requireSha256(header.payloadSha256, "archive transfer acknowledgement payloadSha256");

  // retain the exact unsigned generation and online page bounds
  if (!/^(?:0|[1-9][0-9]*)$/u.test(header.generation) ||
    BigInt(header.generation) > 0xffff_ffff_ffff_ffffn ||
    !Number.isSafeInteger(header.pageIndex) || header.pageIndex < 0 ||
    header.pageIndex >= ADJUSTMENT_CYCLE_MAXIMUM_PAGES ||
    !Number.isSafeInteger(header.payloadOffset) || header.payloadOffset < 0 ||
    !Number.isSafeInteger(value.payloadLength) || value.payloadLength < 1 ||
    value.payloadLength > ADJUSTMENT_CYCLE_PAGE_PAYLOAD_BYTES) {
    throw new TypeError("archive transfer acknowledgement page bounds are invalid");
  }

  // preserve genesis and predecessor-chain header consistency
  if (header.predecessorPageSha256 !== null) {
    requireSha256(
      header.predecessorPageSha256,
      "archive transfer acknowledgement predecessorPageSha256",
    );
  }
  if ((header.pageIndex === 0 &&
      (header.predecessorPageSha256 !== null || header.payloadOffset !== 0)) ||
    (header.pageIndex > 0 && header.predecessorPageSha256 === null)) {
    throw new TypeError("archive transfer acknowledgement predecessor is invalid");
  }

  // require one bounded closed projection list
  if (!Array.isArray(value.projections) || value.projections.length < 1 ||
    value.projections.length > ADJUSTMENT_DAILY_MAXIMUM_PROJECTIONS) {
    throw new TypeError("archive transfer acknowledgement projections are invalid");
  }
  const identities = new Set();

  // validate every exact checkpointed projection identity and channel
  for (const projection of value.projections) {
    requireExactKeys(projection, ["channel", "identitySha256"], "archive projection");
    requireSha256(projection.identitySha256, "archive projection identitySha256");

    // reject unknown channels and duplicate stable projection identities
    if ((projection.channel !== "scheduler_request" && projection.channel !== "organic") ||
      identities.has(projection.identitySha256)) {
      throw new TypeError("archive projection is invalid");
    }
    identities.add(projection.identitySha256);
  }
}

// validate one first-writer genuine final cold-graph receipt
function validateFinalTransferAcknowledgement(value, expectedManifestSha256) {
  requireExactKeys(value, [
    "acknowledgedAt",
    "contractVersion",
    "graphSha256",
    "manifestSha256",
  ], "archive final acknowledgement");
  requireEqual(
    value.contractVersion,
    "adjustment-archive-final-ack/v1",
    "archive final acknowledgement contractVersion",
  );
  requireEqual(
    value.manifestSha256,
    expectedManifestSha256,
    "archive final acknowledgement manifestSha256",
  );
  requireSha256(value.graphSha256, "archive final acknowledgement graphSha256");
  requireInstant(value.acknowledgedAt, "archive final acknowledgement acknowledgedAt");
}

// package the exact committed C metadata and value object bytes into one page payload
async function readCommittedSchedulerPayload(root, prepared) {
  requireSha256(prepared.objectSha256, "prepared.objectSha256");
  const objectGzipBytes = await readRegularFile(
    evidenceObjectPath(root, prepared.objectSha256),
    COMPRESSED_OBJECT_MAXIMUM_BYTES,
  );
  const objectBytes = gunzipBounded(objectGzipBytes);

  // bind the durable compressed member to the exact prepared canonical object
  if (sha256(objectBytes) !== prepared.objectSha256 ||
    !objectBytes.equals(Buffer.from(canonicalJson(prepared.object))) ||
    !gzipSync(objectBytes, { level: 9, mtime: 0 }).equals(objectGzipBytes)) {
    throw new Error("committed scheduler object is invalid");
  }
  validateEvidenceObject(prepared.object);

  const receiptBytes = await readRegularFile(
    evidenceReceiptPath(root, prepared.edgeReceiptIdentitySha256),
    RECEIPT_MAXIMUM_BYTES,
  );
  const receipt = parseReceipt(receiptBytes, prepared.edgeReceiptIdentitySha256);

  // bind the stable receipt to the exact transferred object
  if (receipt.objectSha256 !== prepared.objectSha256 ||
    !receiptBytes.equals(Buffer.from(`${canonicalJson(receipt)}\n`))) {
    throw new Error("committed scheduler receipt is invalid");
  }

  const bindingBytes = await readRegularFile(
    join(
      root,
      "channel-bindings",
      `sha256-${prepared.edgeReceiptIdentitySha256}.json`,
    ),
    RECEIPT_MAXIMUM_BYTES,
  );
  const binding = parseChannelBinding(bindingBytes, prepared.edgeReceiptIdentitySha256);
  const assertionBytes = await readRegularFile(
    join(root, "channels", `sha256-${binding.assertionSha256}.json`),
    RECEIPT_MAXIMUM_BYTES,
  );
  const assertion = parseChannelAssertion(assertionBytes, binding.assertionSha256);

  // archive only an assertion selected by the trusted scheduler request map
  if (assertion.channel !== "scheduler_request" ||
    assertion.receiptSha256 !== sha256(receiptBytes)) {
    throw new Error("committed scheduler channel is invalid");
  }

  const payload = {
    channelAssertionBase64: assertionBytes.toString("base64"),
    channelAssertionSha256: binding.assertionSha256,
    channelBindingBase64: bindingBytes.toString("base64"),
    channelBindingSha256: sha256(bindingBytes),
    contractVersion: "adjustment-evidence-page-payload/v1",
    edgeReceiptIdentitySha256: prepared.edgeReceiptIdentitySha256,
    objectGzipBase64: objectGzipBytes.toString("base64"),
    objectGzipSha256: sha256(objectGzipBytes),
    objectSha256: prepared.objectSha256,
    receiptBase64: receiptBytes.toString("base64"),
    receiptSha256: sha256(receiptBytes),
  };
  return Buffer.from(`${canonicalJson(payload)}\n`);
}

// serialize web capture and forced export operations through one expiring fixed-path lease
async function withArchiveTransportLock(root, now, operation) {
  const directory = join(root, "online-pages", "locks");
  const lockPath = join(directory, "transport.lock");
  const token = randomUUID();
  let acquired = false;
  const lease = {
    contractVersion: "adjustment-archive-lock/v1",
    expiresAt: new Date(now().getTime() + ARCHIVE_LOCK_LEASE_MILLISECONDS).toISOString(),
    token,
  };
  const leaseBytes = Buffer.from(`${canonicalJson(lease)}\n`);

  // wait only for the short bounded capture/export critical section
  for (let attempt = 0; attempt < ARCHIVE_LOCK_ATTEMPTS; attempt += 1) {
    try {
      const handle = await open(
        lockPath,
        fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY |
          fsConstants.O_NOFOLLOW,
        0o600,
      );

      try {
        await handle.writeFile(leaseBytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await fsyncDirectory(directory);
      acquired = true;
      break;
    } catch (error) {
      // fail immediately on every non-contention filesystem error
      if (error?.code !== "EEXIST") {
        throw error;
      }

      const expired = await archiveTransportLockExpired(lockPath, now());

      // atomically remove only an expired lease before retrying acquisition
      if (expired) {
        const stalePath = join(directory, `.stale-${randomUUID()}.lock`);

        try {
          await rename(lockPath, stalePath);
          await unlink(stalePath);
          await fsyncDirectory(directory);
        } catch (renameError) {
          // tolerate only a competing owner changing the fixed lease first
          if (renameError?.code !== "ENOENT") {
            throw renameError;
          }
        }
        continue;
      }

      // yield briefly without permitting an unbounded request backlog
      await new Promise(
        // resume one bounded lease acquisition attempt
        (resolveWait) => setTimeout(resolveWait, ARCHIVE_LOCK_RETRY_MILLISECONDS),
      );
    }
  }

  // refuse rather than bypass a live capture/export owner
  if (!acquired) {
    const error = new Error("archive transport is busy");
    error.code = "archive_busy";
    throw error;
  }

  try {
    return await operation();
  } finally {
    const current = await readRegularFile(lockPath, 2_048).catch(() => null);

    // release only the exact lease created by this operation
    if (current !== null && current.equals(leaseBytes)) {
      await unlink(lockPath);
      await fsyncDirectory(directory);
    }
  }
}

// serialize revision spool mutation across the private web and forced-command processes
async function withRevisionSpoolLock(root, operation) {
  const directory = join(root, "revision-spool-locks");
  const lockPath = join(directory, "spool.lock");
  const token = randomUUID();
  let acquired = false;
  const lease = {
    contractVersion: "adjustment-revision-spool-lock/v1",
    expiresAt: new Date(Date.now() + ARCHIVE_LOCK_LEASE_MILLISECONDS).toISOString(),
    token,
  };
  const leaseBytes = canonicalJsonBytes(lease);
  // wait only for the bounded private spool critical section
  for (let attempt = 0; attempt < ARCHIVE_LOCK_ATTEMPTS; attempt += 1) {
    try {
      const handle = await open(
        lockPath,
        fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY |
          fsConstants.O_NOFOLLOW,
        0o600,
      );
      try {
        await handle.writeFile(leaseBytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await fsyncDirectory(directory);
      acquired = true;
      break;
    } catch (error) {
      // fail every non-contention filesystem error closed
      if (error?.code !== "EEXIST") {
        throw error;
      }
      const expired = await revisionSpoolLockExpired(lockPath);
      // remove only an expired exact spool lease before retrying
      if (expired) {
        const stalePath = join(directory, `.stale-${randomUUID()}.lock`);
        try {
          await rename(lockPath, stalePath);
          await unlink(stalePath);
          await fsyncDirectory(directory);
        } catch (renameError) {
          // tolerate only a competing owner changing the fixed lease first
          if (renameError?.code !== "ENOENT") {
            throw renameError;
          }
        }
        continue;
      }
      await new Promise(
        // yield one short bounded contention interval
        (resolveWait) => setTimeout(resolveWait, ARCHIVE_LOCK_RETRY_MILLISECONDS),
      );
    }
  }
  // refuse mutation rather than bypass a live spool owner
  if (!acquired) {
    const error = new Error("adjustment revision spool is busy");
    error.code = "adjustment_revision_spool_busy";
    throw error;
  }
  try {
    return await operation();
  } finally {
    const current = await readOptionalPrivateFile(lockPath, 2048);
    // release only the exact lease acquired by this operation
    if (current !== null && current.equals(leaseBytes)) {
      await unlink(lockPath);
      await fsyncDirectory(directory);
    }
  }
}

// trust a spool lease expiry only after validating its complete canonical bytes
async function revisionSpoolLockExpired(path) {
  try {
    const bytes = await readRegularFile(path, 2048);
    const value = JSON.parse(bytes.toString("utf8"));
    // accept only the exact fixed lease grammar
    if (value?.contractVersion === "adjustment-revision-spool-lock/v1" &&
      typeof value.token === "string" && /^[a-f0-9-]{36}$/u.test(value.token) &&
      typeof value.expiresAt === "string" && Number.isFinite(Date.parse(value.expiresAt)) &&
      bytes.equals(canonicalJsonBytes(value))) {
      return Date.parse(value.expiresAt) <= Date.now();
    }
  } catch {
    // use file age only for an incomplete crash-left lease
  }
  const details = await lstat(path);
  return details.mtimeMs + ARCHIVE_LOCK_LEASE_MILLISECONDS <= Date.now();
}

// classify a fixed lock as stale only after its complete lease or file age expires
async function archiveTransportLockExpired(path, nowValue) {
  try {
    const bytes = await readRegularFile(path, 2_048);
    const value = JSON.parse(bytes.toString("utf8"));

    // accept only one complete canonical lease when trusting its expiry
    if (value?.contractVersion === "adjustment-archive-lock/v1" &&
      typeof value.token === "string" && /^[a-f0-9-]{36}$/u.test(value.token) &&
      typeof value.expiresAt === "string" &&
      bytes.equals(Buffer.from(`${canonicalJson(value)}\n`)) &&
      Number.isFinite(Date.parse(value.expiresAt))) {
      return Date.parse(value.expiresAt) <= nowValue.getTime();
    }
  } catch {
    // use age only for an incomplete crash-left lease
  }

  const details = await lstat(path);
  return details.mtimeMs + ARCHIVE_LOCK_LEASE_MILLISECONDS <= nowValue.getTime();
}

// calculate the latest eligible jittered due key
export function currentAdjustmentEvidenceDue(nowValue, captureEndAt) {
  const now = new Date(nowValue);
  requireInstant(captureEndAt, "adjustment evidence capture end");

  // reject invalid clocks and stop at the finite boundary
  if (!Number.isFinite(now.getTime()) || now.getTime() >= Date.parse(captureEndAt)) {
    return null;
  }

  const candidates = adjustmentEvidenceDueCandidates(now, captureEndAt);

  // choose only one current due cycle within the catch-up window
  for (let index = candidates.length - 1; index >= 0; index -= 1) {
    const candidate = candidates[index];
    const scheduledTime = Date.parse(candidate.scheduledAt);

    // return the newest due instant and never replay older backlog
    if (scheduledTime <= now.getTime() && now.getTime() - scheduledTime <= SCHEDULER_CATCHUP_MILLISECONDS) {
      return Object.freeze(candidate);
    }
  }
  return null;
}

// evaluate the controller, migration, expiry and measured capacity gates
export async function evaluateAdjustmentEvidenceSchedulerAdmission(input) {
  // require explicit controller activation first
  if (input.enabled !== true) {
    return Object.freeze({ active: false, reason: "controller_inactive" });
  }

  // require the data-plane migration before claiming v2 capture
  if (input.migrationReady !== true) {
    return Object.freeze({ active: false, reason: "migration_unavailable" });
  }

  let schedule;

  try {
    schedule = validateAdjustmentEvidenceScheduleSlots(input.scheduleSlots);
  } catch {
    return Object.freeze({ active: false, reason: "registration_schedule_unavailable" });
  }

  const now = new Date(input.now);

  // close only at the latest authenticated append-only database horizon
  if (!Number.isFinite(now.getTime()) || now.getTime() >= Date.parse(schedule.horizonEndAt)) {
    return Object.freeze({ active: false, captureEndAt: schedule.horizonEndAt,
      reason: "capture_ended" });
  }

  try {
    const filesystem = await input.statfs(input.root);
    const freeBytes = Number(filesystem.bavail * filesystem.bsize);

    // require the protected floor plus one complete next-capture reservation
    if (!Number.isSafeInteger(freeBytes) ||
      freeBytes < ADJUSTMENT_EVIDENCE_SCHEDULER_REQUIRED_FREE_BYTES ||
      Number(filesystem.ffree) < FREE_INODE_FLOOR) {
      return Object.freeze({ active: false, freeBytes, reason: "capacity_blocked" });
    }
    return Object.freeze({ active: true, captureEndAt: schedule.horizonEndAt,
      freeBytes, reason: "active" });
  } catch {
    return Object.freeze({ active: false, reason: "capacity_unavailable" });
  }
}

// validate the three matching database-authenticated rolling schedule projections
export function validateAdjustmentEvidenceScheduleSlots(value) {
  // require one complete closed family projection set
  if (!Array.isArray(value) || value.length !== 3) {
    throw new TypeError("adjustment evidence schedule slots are invalid");
  }
  const families = ["temperature", "wind", "rain"];
  let epochWitnessSha256 = null;
  let horizonEndAt = null;

  // crossbind every family view to the same append-only global horizon head
  for (const [index, slot] of value.entries()) {
    requireExactKeys(slot, [
      "contractVersion", "epochWitnessSha256", "family", "horizonEndAt",
      "registrationSha256", "scheduleContractSha256", "state", "terminalAt",
    ], "adjustment evidence schedule slot");
    requireSha256(slot.epochWitnessSha256,
      "adjustment evidence schedule epoch witness");
    requireInstant(slot.horizonEndAt, "adjustment evidence schedule horizon");
    // validate only a present active registration identity
    if (slot.registrationSha256 !== null) {
      requireSha256(slot.registrationSha256,
        "adjustment evidence schedule registration");
    }
    // admit only the fixed v3 schedule and exact requested family order
    if (slot.contractVersion !== "adjustment-shadow-registration-slot/v3" ||
      slot.family !== families[index] ||
      slot.scheduleContractSha256 !== ADJUSTMENT_ROLLING_SCHEDULE_SHA256 ||
      !["free", "busy_v3", "busy_v2_legacy"].includes(slot.state)) {
      throw new TypeError("adjustment evidence schedule slot is invalid");
    }
    const occupied = slot.state !== "free";
    // bind occupancy clocks without using them as the global horizon authority
    if (occupied !== (slot.registrationSha256 !== null) ||
      occupied !== (slot.terminalAt !== null)) {
      throw new Error("adjustment evidence schedule occupancy differs");
    }
    if (slot.terminalAt !== null) {
      requireInstant(slot.terminalAt, "adjustment evidence schedule terminal");
    }
    epochWitnessSha256 ??= slot.epochWitnessSha256;
    horizonEndAt ??= slot.horizonEndAt;
    // reject three individually valid projections from different schedule heads
    if (slot.epochWitnessSha256 !== epochWitnessSha256 ||
      slot.horizonEndAt !== horizonEndAt) {
      throw new Error("adjustment evidence schedule head differs");
    }
  }
  return Object.freeze({ epochWitnessSha256, horizonEndAt,
    scheduleContractSha256: ADJUSTMENT_ROLLING_SCHEDULE_SHA256 });
}

// normalize only supported direct forecast GET query shapes
export function normalizeAdjustmentEvidenceWindow(requestUrl) {
  const entries = [...requestUrl.searchParams.entries()];

  // retain the dedicated overnight query
  if (entries.length === 1 && entries[0][0] === "window" && entries[0][1] === "overnight") {
    return "overnight";
  }

  // treat the API's empty query as its one-day default
  if (entries.length === 0) {
    return "days=1";
  }

  // retain one reviewed daily query only
  if (entries.length === 1 && entries[0][0] === "days" &&
    ["1", "5", "10"].includes(entries[0][1])) {
    return `days=${entries[0][1]}`;
  }

  return null;
}

// create one deterministic object and its stable identity
export function createAdjustmentEvidenceCapture(filteredBody, window) {
  return createAdjustmentEvidenceCaptureFromForecast(
    JSON.parse(Buffer.from(filteredBody).toString("utf8")),
    window,
  );
}

// create one deterministic object from an already parsed forecast
export function createAdjustmentEvidenceCaptureFromForecast(
  forecast,
  window,
  compressCanonicalObject = compressEvidenceObject,
) {
  return createAdjustmentEvidenceCaptureVersion(forecast, window, {
    contractVersion: ADJUSTMENT_EVIDENCE_OBJECT_CONTRACT_VERSION,
    maximumRows: MAXIMUM_ROWS,
  }, compressCanonicalObject);
}

// create one v2 object that admits the fall-back DST row
export function createAdjustmentEvidenceCaptureV2(filteredBody, window) {
  return createAdjustmentEvidenceCaptureV2FromForecast(
    JSON.parse(Buffer.from(filteredBody).toString("utf8")),
    window,
  );
}

// create one v2 object from an already parsed forecast
export function createAdjustmentEvidenceCaptureV2FromForecast(
  forecast,
  window,
  compressCanonicalObject = compressEvidenceObject,
) {
  return createAdjustmentEvidenceCaptureVersion(forecast, window, {
    contractVersion: ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION,
    maximumRows: MAXIMUM_V2_ROWS,
  }, compressCanonicalObject);
}

// create one versioned deterministic object and stable receipt identity
function createAdjustmentEvidenceCaptureVersion(
  forecast,
  window,
  version,
  compressCanonicalObject,
) {
  if (!WINDOWS.includes(window)) {
    throw new TypeError("adjustment evidence window is invalid");
  }

  // require one successful filtered forecast envelope
  if (forecast === null || typeof forecast !== "object" || !Array.isArray(forecast.data) ||
    forecast.data.length < 1 || forecast.data.length > version.maximumRows) {
    throw new TypeError("forecast evidence body is invalid");
  }

  const settingsSha256 = sha256(canonicalJson(forecast.adjustmentSettings));
  const bundleIdentities = projectBundleIdentities(forecast);
  const availability = projectAvailability(forecast.data);
  const rows = forecast.data.map(
    // preserve response order for row-to-availability joins
    (row) => projectEvidenceRow(row),
  );
  const object = {
    bundleIdentities,
    contractVersion: version.contractVersion,
    rows,
    settingsSha256,
    siteKey: "ballydidean",
    window,
  };
  const canonicalObject = Buffer.from(canonicalJson(object));

  // bound canonical bytes before compression
  if (canonicalObject.byteLength > OBJECT_MAXIMUM_BYTES) {
    throw objectTooLargeError();
  }

  const compressed = compressCanonicalObject(canonicalObject);
  validateCompressedEvidenceObject(canonicalObject, compressed.compressedObject);

  const stableIdentity = {
    bundleIdentities,
    rows: rows.map(
      // exclude all scoring values from the stable record identity
      (row) => ({ record: row.record, source: row.source }),
    ),
    settingsSha256,
    siteKey: "ballydidean",
    window,
  };

  return Object.freeze({
    availability,
    compressedObject: Buffer.from(compressed.compressedObject),
    edgeReceiptIdentitySha256: sha256(canonicalJson(stableIdentity)),
    object,
    objectSha256: compressed.objectSha256,
  });
}

// compress one canonical object without retaining it
function compressEvidenceObject(canonicalObject) {
  const compressedObject = gzipSync(canonicalObject, { level: 9, mtime: 0 });
  validateCompressedEvidenceObject(canonicalObject, compressedObject);
  return { compressedObject, objectSha256: sha256(canonicalObject) };
}

// require one bounded deterministic compressed object
function validateCompressedEvidenceObject(canonicalObject, compressedObject) {
  if (!Buffer.isBuffer(compressedObject) ||
    compressedObject.byteLength > COMPRESSED_OBJECT_MAXIMUM_BYTES ||
    canonicalObject.byteLength > compressedObject.byteLength * MAXIMUM_COMPRESSION_RATIO) {
    throw objectTooLargeError();
  }
}

// freeze and validate every receipt selected at snapshot start
export async function freezeAdjustmentEvidenceSnapshot(options = {}) {
  const root = resolve(options.root ?? ADJUSTMENT_EVIDENCE_DEFAULT_ROOT);
  const now = options.now ?? (() => new Date());
  await validateEvidenceDirectories(root);
  const receiptDirectory = join(root, "receipts");
  const names = (await readdir(receiptDirectory)).sort();

  // reject unbounded or unexpected receipt directory contents
  if (names.length > MAXIMUM_IDENTITIES || names.some((name) => !RECEIPT_FILENAME_PATTERN.test(name))) {
    throw new Error("adjustment evidence receipt directory is invalid");
  }

  const entries = [];

  // validate the frozen list without observing later receipts
  for (const name of names) {
    const identity = RECEIPT_FILENAME_PATTERN.exec(name)?.[1];
    const receiptPath = evidenceReceiptPath(root, identity);
    const receipt = await readReceipt(receiptPath, identity);
    const objectPath = evidenceObjectPath(root, receipt.objectSha256);
    const objectBytes = await readRegularFile(
      objectPath,
      COMPRESSED_OBJECT_MAXIMUM_BYTES,
    );
    const canonicalObject = gunzipBounded(objectBytes);

    // bind compressed bytes to the content-addressed canonical object
    if (sha256(canonicalObject) !== receipt.objectSha256) {
      throw new Error("adjustment evidence object hash is invalid");
    }

    const object = JSON.parse(canonicalObject.toString("utf8"));
    validateEvidenceObject(object);

    // require canonical deterministic content at the export boundary
    if (!Buffer.from(canonicalJson(object)).equals(canonicalObject) ||
      !gzipSync(canonicalObject, { level: 9, mtime: 0 }).equals(objectBytes)) {
      throw new Error("adjustment evidence object encoding is invalid");
    }

    // bind row availability and stable identity to the selected object
    if (receiptAvailability(receipt).rowTimestampIndexes.length !== object.rows.length ||
      stableIdentitySha256(object) !== identity ||
      receipt.window !== object.window ||
      receipt.contractVersion !== receiptVersionForObject(object.contractVersion)) {
      throw new Error("adjustment evidence receipt pairing is invalid");
    }
    entries.push({
      edgeReceiptIdentitySha256: identity,
      objectPath,
      objectSha256: receipt.objectSha256,
      receiptPath,
    });
  }

  const watermarkSha256 = sha256(canonicalJson(entries.map(
    // exclude host paths from the portable watermark
    (entry) => ({
      edgeReceiptIdentitySha256: entry.edgeReceiptIdentitySha256,
      objectSha256: entry.objectSha256,
    }),
  )));
  return Object.freeze({
    contractVersion: ADJUSTMENT_EVIDENCE_SNAPSHOT_CONTRACT_VERSION,
    entries: Object.freeze(entries),
    frozenAt: now().toISOString(),
    watermarkSha256,
  });
}

// create the first-observed availability receipt
function createReceipt(prepared, firstEdgeCommittedAt) {
  // encode v2 availability compactly without changing v1 receipt bytes
  if (prepared.object.contractVersion === ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION) {
    const availabilityBytes = Buffer.from(canonicalJson(prepared.availability));
    return {
      availabilityGzipBase64: gzipSync(availabilityBytes, { level: 9, mtime: 0 }).toString("base64"),
      contractVersion: ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION,
      edgeReceiptIdentitySha256: prepared.edgeReceiptIdentitySha256,
      firstEdgeCommittedAt,
      objectSha256: prepared.objectSha256,
      siteKey: "ballydidean",
      window: prepared.object.window,
    };
  }

  return {
    availability: prepared.availability,
    contractVersion: ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION,
    edgeReceiptIdentitySha256: prepared.edgeReceiptIdentitySha256,
    firstEdgeCommittedAt,
    objectSha256: prepared.objectSha256,
    siteKey: "ballydidean",
    window: prepared.object.window,
  };
}

// retain only active response-level serving identities
function projectBundleIdentities(forecast) {
  return {
    rain: {
      activeBundle: nullableSha256(forecast.rainAdjustmentRuntime?.activeBundle),
    },
    temperature: {
      activeBundle: nullableSha256(forecast.temperatureAdjustmentRuntime?.activeBundle),
      authorizationSha256: nullableSha256(
        forecast.temperatureAdjustmentRuntime?.authorizationSha256,
      ),
    },
    wind: {
      activeBundle: nullableSha256(forecast.adjustmentRuntime?.activeBundle),
      authorizationSha256: nullableSha256(forecast.adjustmentRuntime?.authorizationSha256),
      candidateArtifactSha256: nullableSha256(
        forecast.adjustmentRuntime?.candidateArtifactSha256,
      ),
    },
  };
}

// group mutable first-observed timestamps by ordered row index
function projectAvailability(rows) {
  const indexes = new Map();
  const timestamps = [];
  const rowTimestampIndexes = rows.map(
    // retain each distinct mutable timestamp only once
    (row) => {
      requireInstant(row?.receivedAt, "row.receivedAt");
      let index = indexes.get(row.receivedAt);

      // add one first-seen canonical timestamp
      if (index === undefined) {
        index = timestamps.length;
        indexes.set(row.receivedAt, index);
        timestamps.push(row.receivedAt);
      }

      return index;
    },
  );
  return { rowTimestampIndexes, timestamps };
}

// project one closed scoring row without volatile response metadata
function projectEvidenceRow(row) {
  if (row === null || typeof row !== "object") {
    throw new TypeError("forecast evidence row is invalid");
  }

  return {
    provenanceComplete: false,
    rainAdjustment: projectRainAdjustment(row.rainAdjustment),
    raw: projectMetricValues(row.metrics, RAW_METRICS, "row.metrics"),
    record: {
      id: requireBoundedString(row.id, "row.id"),
      productRunAt: nullableInstant(row.productRunAt, "row.productRunAt"),
      revisionCount: requireRevisionCount(row.revisionCount),
      validAt: instant(row.validAt, "row.validAt"),
    },
    source: {
      dataset: nullableBoundedString(row.metadata?.provider?.dataset, "row.metadata.provider.dataset"),
      providerKey: requireBoundedString(row.provenance?.providerKey, "row.provenance.providerKey"),
      sourceId: requireBoundedString(row.provenance?.sourceId, "row.provenance.sourceId"),
      sourceKey: requireBoundedString(row.provenance?.sourceKey, "row.provenance.sourceKey"),
      upstreamModel: nullableBoundedString(row.metadata?.upstream?.model, "row.metadata.upstream.model"),
    },
    temperatureAdjustment: projectTemperatureAdjustment(row.temperatureAdjustment),
    windAdjustment: projectWindAdjustment(row.adjustment),
  };
}

// retain post-settings generic decision fields only
function projectWindAdjustment(value) {
  const decision = requireDecision(value, "row.adjustment");
  const adjustedMetrics = projectMetricValues(
    decision.adjustedMetrics ?? {},
    ADJUSTED_METRICS,
    "row.adjustment.adjustedMetrics",
    true,
  );
  return {
    adjustedMetrics,
    appliedMetrics: requireStringArray(decision.appliedMetrics ?? [], "row.adjustment.appliedMetrics"),
    authorizationSha256: nullableSha256(decision.authorizationSha256),
    candidateArtifactSha256: nullableSha256(decision.candidateArtifactSha256),
    leadBand: nullableBoundedString(decision.leadBand, "row.adjustment.leadBand"),
    reasonCode: nullableBoundedString(decision.reasonCode, "row.adjustment.reasonCode"),
    state: requireState(decision.state, "row.adjustment.state", WIND_DECISION_STATES),
  };
}

// retain temperature amount, reason and immutable source receipt fields
function projectTemperatureAdjustment(value) {
  const decision = requireDecision(value, "row.temperatureAdjustment");
  return {
    branch: nullableBoundedString(decision.branch, "row.temperatureAdjustment.branch"),
    bundleSha256: nullableSha256(decision.bundleSha256),
    correctedTemperatureC: nullableFinite(
      decision.correctedTemperatureC,
      "row.temperatureAdjustment.correctedTemperatureC",
    ),
    rawBestMatchTemperatureC: nullableFinite(
      decision.rawBestMatchTemperatureC,
      "row.temperatureAdjustment.rawBestMatchTemperatureC",
    ),
    reasonCode: nullableBoundedString(
      decision.reasonCode,
      "row.temperatureAdjustment.reasonCode",
    ),
    sourceForecast: projectTemperatureSource(decision.sourceForecast),
    state: requireState(decision.state, "row.temperatureAdjustment.state", SOURCE_DECISION_STATES),
  };
}

// retain only the closed temperature source forecast
function projectTemperatureSource(value) {
  if (value === null || value === undefined) {
    return null;
  }

  requirePlainObject(value, "row.temperatureAdjustment.sourceForecast");
  return {
    adapterVersion: requireBoundedString(value.adapterVersion, "temperature.source.adapterVersion"),
    dataset: requireBoundedString(value.dataset, "temperature.source.dataset"),
    firstReceivedAt: instant(value.firstReceivedAt, "temperature.source.firstReceivedAt"),
    modelCycle: requireBoundedString(value.modelCycle, "temperature.source.modelCycle"),
    modelLeadHours: requireBoundedInteger(value.modelLeadHours, "temperature.source.modelLeadHours"),
    operationalHorizonHours: requireBoundedInteger(
      value.operationalHorizonHours,
      "temperature.source.operationalHorizonHours",
    ),
    providerKey: requireBoundedString(value.providerKey, "temperature.source.providerKey"),
    providerResponseSha256: requireSha256(value.providerResponseSha256, "temperature.source.providerResponseSha256"),
    rawRelativeHumidityPercent: nullableFinite(value.rawRelativeHumidityPercent, "temperature.source.rawRelativeHumidityPercent"),
    rawTemperatureC: finite(value.rawTemperatureC, "temperature.source.rawTemperatureC"),
    rawWindSpeedMps: nullableFinite(value.rawWindSpeedMps, "temperature.source.rawWindSpeedMps"),
    runInitializedAt: instant(value.runInitializedAt, "temperature.source.runInitializedAt"),
    upstreamModel: requireBoundedString(value.upstreamModel, "temperature.source.upstreamModel"),
    validAt: instant(value.validAt, "temperature.source.validAt"),
  };
}

// retain rain amount, reason and immutable source receipt fields
function projectRainAdjustment(value) {
  const decision = requireDecision(value, "row.rainAdjustment");
  return {
    bundleSha256: nullableSha256(decision.bundleSha256),
    correctedPrecipitationMm: nullableFinite(
      decision.correctedPrecipitationMm,
      "row.rainAdjustment.correctedPrecipitationMm",
    ),
    rawBestMatchPrecipitationMm: nullableFinite(
      decision.rawBestMatchPrecipitationMm,
      "row.rainAdjustment.rawBestMatchPrecipitationMm",
    ),
    reasonCode: nullableBoundedString(decision.reasonCode, "row.rainAdjustment.reasonCode"),
    sourceForecast: projectRainSource(decision.sourceForecast),
    state: requireState(decision.state, "row.rainAdjustment.state", SOURCE_DECISION_STATES),
  };
}

// retain only the closed rain source forecast
function projectRainSource(value) {
  if (value === null || value === undefined) {
    return null;
  }

  requirePlainObject(value, "row.rainAdjustment.sourceForecast");
  return {
    decisionAt: instant(value.decisionAt, "rain.source.decisionAt"),
    firstReceivedAt: instant(value.firstReceivedAt, "rain.source.firstReceivedAt"),
    modelLeadHours: requireBoundedInteger(value.modelLeadHours, "rain.source.modelLeadHours"),
    providerKey: requireBoundedString(value.providerKey, "rain.source.providerKey"),
    rawPrecipitationMm: finite(value.rawPrecipitationMm, "rain.source.rawPrecipitationMm"),
    runInitializedAt: instant(value.runInitializedAt, "rain.source.runInitializedAt"),
    upstreamModel: requireBoundedString(value.upstreamModel, "rain.source.upstreamModel"),
    validAt: instant(value.validAt, "rain.source.validAt"),
  };
}

// retain a fixed metric allowlist with nulls for absent raw values
function projectMetricValues(value, keys, path, omitAbsent = false) {
  requirePlainObject(value, path);
  const projected = {};

  // preserve the reviewed metric order
  for (const key of keys) {
    if (!omitAbsent || Object.hasOwn(value, key)) {
      projected[key] = nullableFinite(value[key] ?? null, `${path}.${key}`);
    }
  }

  return projected;
}

// scan bounded immutable directories and allocation
async function scanEvidenceDirectories(root, validatePairs) {
  await validateEvidenceDirectories(root);
  const objects = await readdir(join(root, "objects"));
  const receipts = await readdir(join(root, "receipts"));
  const channels = await readdir(join(root, "channels"));
  const channelBindings = await readdir(join(root, "channel-bindings"));

  // stop startup on unexpected or unbounded contents
  if (objects.length > MAXIMUM_IDENTITIES || receipts.length > MAXIMUM_IDENTITIES ||
    objects.some((name) => !OBJECT_FILENAME_PATTERN.test(name)) ||
    receipts.some((name) => !RECEIPT_FILENAME_PATTERN.test(name)) ||
    channels.length > MAXIMUM_CHANNEL_ASSERTIONS ||
    channelBindings.length > MAXIMUM_CHANNEL_ASSERTIONS ||
    channels.some((name) => !CHANNEL_FILENAME_PATTERN.test(name)) ||
    channelBindings.some((name) => !RECEIPT_FILENAME_PATTERN.test(name))) {
    throw new Error("adjustment evidence directory is invalid");
  }

  let allocatedBytes = 0;

  // count real allocation without following symlinks
  for (const [directory, names] of [
    ["objects", objects],
    ["receipts", receipts],
    ["channels", channels],
    ["channel-bindings", channelBindings],
  ]) {
    for (const name of names) {
      const details = await lstat(join(root, directory, name), { bigint: true });

      // reject links, devices and oversized files
      if (!details.isFile() || details.isSymbolicLink() ||
        (Number(details.mode) & 0o777) !== 0o600 ||
        details.size > BigInt(directory === "objects"
          ? COMPRESSED_OBJECT_MAXIMUM_BYTES
          : RECEIPT_MAXIMUM_BYTES)) {
        throw new Error("adjustment evidence entry is invalid");
      }

      allocatedBytes += Number(details.blocks * 512n);
    }
  }

  // reserve optional exhaustive validation for export
  if (validatePairs) {
    // validate every object, including a retained orphan from a failed receipt
    for (const name of objects) {
      const expectedSha256 = OBJECT_FILENAME_PATTERN.exec(name)?.[1];
      const bytes = await readRegularFile(
        join(root, "objects", name),
        COMPRESSED_OBJECT_MAXIMUM_BYTES,
      );
      const canonical = gunzipBounded(bytes);
      const object = JSON.parse(canonical.toString("utf8"));

      if (sha256(canonical) !== expectedSha256 ||
        !Buffer.from(canonicalJson(object)).equals(canonical) ||
        !gzipSync(canonical, { level: 9, mtime: 0 }).equals(bytes)) {
        throw new Error("adjustment evidence object is corrupt");
      }

      validateEvidenceObject(object);
    }

    // verify every content-addressed issuance-channel assertion
    for (const name of channels) {
      const expectedSha256 = CHANNEL_FILENAME_PATTERN.exec(name)?.[1];
      const bytes = await readRegularFile(join(root, "channels", name), RECEIPT_MAXIMUM_BYTES);
      parseChannelAssertion(bytes, expectedSha256);
    }

    // verify every stable binding and its exact assertion target
    for (const name of channelBindings) {
      const expectedIdentity = RECEIPT_FILENAME_PATTERN.exec(name)?.[1];
      const bytes = await readRegularFile(
        join(root, "channel-bindings", name),
        RECEIPT_MAXIMUM_BYTES,
      );
      const binding = parseChannelBinding(bytes, expectedIdentity);
      const assertionBytes = await readRegularFile(
        join(root, "channels", `sha256-${binding.assertionSha256}.json`),
        RECEIPT_MAXIMUM_BYTES,
      );
      const assertion = parseChannelAssertion(assertionBytes, binding.assertionSha256);
      const receiptBytes = await readRegularFile(
        evidenceReceiptPath(root, expectedIdentity),
        RECEIPT_MAXIMUM_BYTES,
      );
      const receipt = parseReceipt(receiptBytes, expectedIdentity);

      // bind the pointer filename to the assertion's stable receipt identity
      if (assertion.edgeReceiptIdentitySha256 !== expectedIdentity ||
        assertion.receiptSha256 !== sha256(receiptBytes) ||
        assertion.firstEdgeCommittedAt !== receipt.firstEdgeCommittedAt) {
        throw new Error("adjustment evidence channel pairing is invalid");
      }
    }

    await freezeAdjustmentEvidenceSnapshot({ root });
  }

  return {
    allocatedBytes,
    channelAssertionCount: channelBindings.length,
    objectCount: objects.length,
    receiptCount: receipts.length,
  };
}

// create fixed evidence directories and validate real paths
async function ensureEvidenceDirectories(root) {
  await ensurePrivateDirectory(root);

  // create only fixed private children beneath the validated real root
  for (const directory of ["objects", "receipts", "channels", "channel-bindings"]) {
    await ensurePrivateDirectory(join(root, directory));
  }
  await validateEvidenceDirectories(root);
}

// create one private directory without following or changing existing paths
async function ensurePrivateDirectory(path) {
  const parentPath = dirname(path);
  const canonicalParent = await realpath(parentPath);

  // reject configured ancestors that resolve through links
  if (canonicalParent !== resolve(parentPath)) {
    throw new Error("private directory parent is invalid");
  }

  try {
    await mkdir(path, { mode: 0o700 });
  } catch (error) {
    // accept only an already existing path for exact validation
    if (error?.code !== "EEXIST") {
      throw error;
    }
  }
  await requirePrivateDirectory(path, canonicalParent);
}

// create one service-owned private directory without repairing existing paths
async function ensureOwnedPrivateDirectory(path, expectedUid, expectedGid) {
  const parentPath = dirname(path);
  const canonicalParent = await realpath(parentPath);

  // reject configured ancestors that resolve through links
  if (canonicalParent !== resolve(parentPath)) {
    throw new Error("owned private directory parent is invalid");
  }
  let created = false;
  try {
    await mkdir(path, { mode: 0o700 });
    created = true;
  } catch (error) {
    // accept only an already existing path for exact validation
    if (error?.code !== "EEXIST") {
      throw error;
    }
  }

  // assign service ownership only to a directory created by this initializer
  if (created) {
    await chown(path, expectedUid, expectedGid);
  }
  await requireOwnedPrivateDirectory(path, expectedUid, expectedGid, canonicalParent);
}

// require one canonical private directory with exact service ownership
async function requireOwnedPrivateDirectory(
  path,
  expectedUid,
  expectedGid,
  expectedParent = null,
) {
  const details = await lstat(path);
  const canonicalPath = await realpath(path);

  // reject links, aliases, broad modes and foreign ownership
  if (!details.isDirectory() || details.isSymbolicLink() ||
    (details.mode & 0o777) !== 0o700 || details.uid !== expectedUid ||
    details.gid !== expectedGid || canonicalPath !== resolve(path) ||
    (expectedParent !== null && dirname(canonicalPath) !== expectedParent)) {
    throw new Error("owned private directory is invalid");
  }
}

// require one ordinary numeric account identity
function requireId(value, label) {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new TypeError(`${label} is invalid`);
  }
}

// require one real owner-private directory at its literal path
async function requirePrivateDirectory(path, expectedParent = null) {
  const details = await lstat(path);
  const canonicalPath = await realpath(path);

  // reject links, aliases and permissions not already owner-private
  if (!details.isDirectory() || details.isSymbolicLink() ||
    (details.mode & 0o777) !== 0o700 || canonicalPath !== resolve(path) ||
    (expectedParent !== null && dirname(canonicalPath) !== expectedParent)) {
    throw new Error("private directory is invalid");
  }
}

// reject directory links and path escapes
async function validateEvidenceDirectories(root) {
  const rootDetails = await lstat(root);

  // require a real private root directory
  if (!rootDetails.isDirectory() || rootDetails.isSymbolicLink() ||
    (rootDetails.mode & 0o777) !== 0o700) {
    throw new Error("adjustment evidence root is invalid");
  }

  const canonicalRoot = await realpath(root);

  // bind both fixed child directories beneath the canonical root
  for (const directory of ["objects", "receipts", "channels", "channel-bindings"]) {
    const path = join(root, directory);
    const details = await lstat(path);
    const canonicalPath = await realpath(path);

    if (!details.isDirectory() || details.isSymbolicLink() ||
      (details.mode & 0o777) !== 0o700 ||
      dirname(canonicalPath) !== canonicalRoot) {
      throw new Error("adjustment evidence directory is invalid");
    }
  }
}

// create or verify one immutable content object
async function writeOrVerifyImmutable(path, bytes, maximumBytes, writer) {
  try {
    await writer(path, bytes);
    return true;
  } catch (error) {
    // verify an existing content-addressed object byte for byte
    if (error?.code !== "EEXIST") {
      throw error;
    }

    const existing = await readRegularFile(path, maximumBytes);

    if (!existing.equals(bytes)) {
      throw new Error("adjustment evidence object collision");
    }

    return false;
  }
}

// create or verify one exclusive stable-identity receipt
async function writeOrVerifyReceipt(path, bytes, prepared, writer) {
  try {
    await writer(path, bytes);
    return true;
  } catch (error) {
    // leave an existing first receipt authoritative
    if (error?.code !== "EEXIST") {
      throw error;
    }

    const existing = await readReceipt(path, prepared.edgeReceiptIdentitySha256);

    if (existing.objectSha256 !== prepared.objectSha256) {
      return false;
    }

    return false;
  }
}

// durably publish one complete private file through an exclusive hard link
async function writeExclusive(path, bytes) {
  const directoryPath = dirname(path);
  const temporaryPath = join(directoryPath, `.capture-${randomUUID()}.tmp`);
  let handle = await open(
    temporaryPath,
    fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW,
    0o600,
  );
  let linked = false;
  let temporaryExists = true;

  try {
    await handle.writeFile(bytes);
    await handle.sync();
    await handle.close();
    handle = null;
    await link(temporaryPath, path);
    linked = true;
    await unlink(temporaryPath);
    temporaryExists = false;

    const directory = await open(
      directoryPath,
      fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW,
    );

    try {
      await directory.sync();
    } finally {
      await directory.close();
    }
  } catch (error) {
    // close only the still-open private temporary handle
    if (handle !== null) {
      await handle.close().catch(() => undefined);
    }

    // remove only the uniquely named temporary created by this call
    if (temporaryExists) {
      try {
        await unlink(temporaryPath);
        temporaryExists = false;
      } catch (cleanupError) {
        // ignore only a path already removed by the publication flow
        if (cleanupError?.code !== "ENOENT") {
          error.adjustmentEvidenceTemporaryCleanupFailed = true;
        }
      }
    }

    // disclose only whether the complete final link exists for reconciliation
    if (linked) {
      error.adjustmentEvidenceFinalCreated = true;
    }
    throw error;
  }
}

// read and validate one first-issuance receipt
async function readReceipt(path, expectedIdentity) {
  const bytes = await readRegularFile(path, RECEIPT_MAXIMUM_BYTES);
  return parseReceipt(bytes, expectedIdentity);
}

// parse one receipt while preserving both exact reader contracts
function parseReceipt(bytes, expectedIdentity) {
  const value = JSON.parse(bytes.toString("utf8"));
  validateReceipt(value, expectedIdentity);

  // enforce the closed v2 receipt ceiling at every reader
  if (value.contractVersion === ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION &&
    bytes.byteLength > 2_048) {
    throw new TypeError("v2 receipt is too large");
  }
  return value;
}

// read one bounded regular file without following links
async function readRegularFile(path, maximumBytes) {
  const handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);

  try {
    const details = await handle.stat();

    if (!details.isFile() || (details.mode & 0o777) !== 0o600 ||
      details.size > maximumBytes) {
      throw new Error("adjustment evidence file is invalid");
    }

    return await handle.readFile();
  } finally {
    await handle.close();
  }
}

// validate one receipt without accepting unknown fields
function validateReceipt(value, expectedIdentity) {
  const maximumRows = value.contractVersion === ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION
    ? MAXIMUM_ROWS
    : value.contractVersion === ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION
      ? MAXIMUM_V2_ROWS
      : null;

  // accept only the immutable v1 and v2 receipt schemas
  if (maximumRows === null) {
    throw new TypeError("receipt.contractVersion is invalid");
  }

  // require the exact versioned receipt keys
  requireExactKeys(value, value.contractVersion === ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION
    ? [
        "availabilityGzipBase64",
        "contractVersion",
        "edgeReceiptIdentitySha256",
        "firstEdgeCommittedAt",
        "objectSha256",
        "siteKey",
        "window",
      ]
    : [
        "availability",
        "contractVersion",
        "edgeReceiptIdentitySha256",
        "firstEdgeCommittedAt",
        "objectSha256",
        "siteKey",
        "window",
      ], "receipt");
  requireEqual(value.edgeReceiptIdentitySha256, expectedIdentity, "receipt.edgeReceiptIdentitySha256");
  requireSha256(value.objectSha256, "receipt.objectSha256");
  requireInstant(value.firstEdgeCommittedAt, "receipt.firstEdgeCommittedAt");
  requireEqual(value.siteKey, "ballydidean", "receipt.siteKey");

  if (!WINDOWS.includes(value.window)) {
    throw new TypeError("receipt.window is invalid");
  }

  validateReceiptAvailability(receiptAvailability(value), maximumRows);
}

// recover one receipt's versioned availability projection
function receiptAvailability(receipt) {
  // preserve the exact readable v1 object
  if (receipt.contractVersion === ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION) {
    return receipt.availability;
  }

  if (typeof receipt.availabilityGzipBase64 !== "string" ||
    receipt.availabilityGzipBase64.length > 2_048) {
    throw new TypeError("receipt availability encoding is invalid");
  }
  const compressed = Buffer.from(receipt.availabilityGzipBase64, "base64");

  // reject noncanonical base64 spellings
  if (compressed.toString("base64") !== receipt.availabilityGzipBase64) {
    throw new TypeError("receipt availability encoding is invalid");
  }
  const canonical = gunzipSync(compressed, { maxOutputLength: 32 * 1_024 });
  const availability = JSON.parse(canonical.toString("utf8"));

  // require deterministic compact bytes for the v2 reader
  if (!canonical.equals(Buffer.from(canonicalJson(availability))) ||
    !gzipSync(canonical, { level: 9, mtime: 0 }).equals(compressed)) {
    throw new TypeError("receipt availability encoding is invalid");
  }
  return availability;
}

// validate one grouped first-observed availability projection
function validateReceiptAvailability(availability, maximumRows) {
  requireExactKeys(availability, ["rowTimestampIndexes", "timestamps"], "receipt.availability");

  if (!Array.isArray(availability.timestamps) ||
    !Array.isArray(availability.rowTimestampIndexes) ||
    availability.rowTimestampIndexes.length > maximumRows) {
    throw new TypeError("receipt availability is invalid");
  }

  // validate all grouped timestamps
  for (const timestamp of availability.timestamps) {
    requireInstant(timestamp, "receipt.availability.timestamps[]");
  }

  // validate every row timestamp reference
  for (const index of availability.rowTimestampIndexes) {
    if (!Number.isSafeInteger(index) || index < 0 || index >= availability.timestamps.length) {
      throw new TypeError("receipt availability index is invalid");
    }
  }
}

// validate the outer object and row count before export
function validateEvidenceObject(value) {
  requireExactKeys(value, [
    "bundleIdentities",
    "contractVersion",
    "rows",
    "settingsSha256",
    "siteKey",
    "window",
  ], "object");
  const maximumRows = value.contractVersion === ADJUSTMENT_EVIDENCE_OBJECT_CONTRACT_VERSION
    ? MAXIMUM_ROWS
    : value.contractVersion === ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION
      ? MAXIMUM_V2_ROWS
      : null;

  // accept only the immutable v1 and v2 object schemas
  if (maximumRows === null) {
    throw new TypeError("object.contractVersion is invalid");
  }
  requireSha256(value.settingsSha256, "object.settingsSha256");
  requireEqual(value.siteKey, "ballydidean", "object.siteKey");

  if (!WINDOWS.includes(value.window) || !Array.isArray(value.rows) ||
    value.rows.length < 1 || value.rows.length > maximumRows) {
    throw new TypeError("adjustment evidence object is invalid");
  }
}

// decompress within the canonical and ratio limits
function gunzipBounded(compressed) {
  const canonical = gunzipSync(compressed, {
    finishFlush: 4,
    maxOutputLength: OBJECT_MAXIMUM_BYTES + 1,
  });

  if (canonical.byteLength > OBJECT_MAXIMUM_BYTES ||
    canonical.byteLength > compressed.byteLength * MAXIMUM_COMPRESSION_RATIO) {
    throw new Error("adjustment evidence object exceeds decompression limits");
  }

  return canonical;
}

// calculate one content-addressed object path
function evidenceObjectPath(root, sha256Value) {
  requireSha256(sha256Value, "objectSha256");
  return join(root, "objects", `sha256-${sha256Value}.json.gz`);
}

// calculate one stable-identity receipt path
function evidenceReceiptPath(root, sha256Value) {
  requireSha256(sha256Value, "edgeReceiptIdentitySha256");
  return join(root, "receipts", `sha256-${sha256Value}.json`);
}

// detect one existing regular path without accepting invalid entries
async function regularFileExists(path) {
  try {
    const details = await lstat(path);

    if (!details.isFile() || details.isSymbolicLink()) {
      throw new Error("adjustment evidence path is invalid");
    }

    return true;
  } catch (error) {
    // treat only a missing path as available for exclusive creation
    if (error?.code === "ENOENT") {
      return false;
    }

    throw error;
  }
}

// calculate conservative filesystem allocation
function allocatedSize(bytes, blockSize) {
  return Math.ceil(bytes / blockSize) * blockSize;
}

// create one empty bounded scheduler state
function createSchedulerState() {
  return {
    activation: "not_initialized",
    attempts: [],
    contractVersion: ADJUSTMENT_EVIDENCE_SCHEDULER_STATE_CONTRACT_VERSION,
    errors: [],
    gaps: [],
    lastCheckedAt: null,
    updatedAt: null,
  };
}

// validate one closed scheduler restart-state document
function validateSchedulerState(state) {
  requireExactKeys(state, [
    "activation",
    "attempts",
    "contractVersion",
    "errors",
    "gaps",
    "lastCheckedAt",
    "updatedAt",
  ], "scheduler state");
  requireEqual(
    state.contractVersion,
    ADJUSTMENT_EVIDENCE_SCHEDULER_STATE_CONTRACT_VERSION,
    "scheduler state contractVersion",
  );

  // enforce bounded status strings and history arrays
  if (typeof state.activation !== "string" || state.activation.length > 64 ||
    !Array.isArray(state.attempts) || state.attempts.length > SCHEDULER_HISTORY_LIMIT ||
    !Array.isArray(state.errors) || state.errors.length > SCHEDULER_HISTORY_LIMIT ||
    !Array.isArray(state.gaps) || state.gaps.length > SCHEDULER_HISTORY_LIMIT) {
    throw new TypeError("scheduler state is invalid");
  }

  // validate optional state instants
  for (const instantValue of [state.lastCheckedAt, state.updatedAt]) {
    if (instantValue !== null) {
      requireInstant(instantValue, "scheduler state instant");
    }
  }

  // validate every bounded capture attempt
  for (const attempt of state.attempts) {
    requireExactKeys(attempt, ["committedAt", "dueKey", "status"], "scheduler attempt");
    requireInstant(attempt.committedAt, "scheduler attempt committedAt");
    requireSchedulerDueKey(attempt.dueKey);
    requireBoundedString(attempt.status, "scheduler attempt status");
  }

  // validate every bounded scheduler error
  for (const error of state.errors) {
    requireExactKeys(error, ["at", "code", "dueKey"], "scheduler error");
    requireInstant(error.at, "scheduler error at");
    requireBoundedString(error.code, "scheduler error code");
    requireSchedulerDueKey(error.dueKey);
  }

  // validate every durable categorical scheduler gap
  for (const gap of state.gaps) {
    requireExactKeys(gap, ["dueKey", "occurredAt", "reason"], "scheduler gap");
    requireSchedulerDueKey(gap.dueKey);
    requireInstant(gap.occurredAt, "scheduler gap occurredAt");

    // retain only the two scheduler refusal categories
    if (gap.reason !== "missed_capture" && gap.reason !== "capture_failed") {
      throw new TypeError("scheduler gap reason is invalid");
    }
  }
  return state;
}

// create nearby deterministic jittered due candidates
function adjustmentEvidenceDueCandidates(now, captureEndAt) {
  const candidates = [];

  // inspect yesterday and today to cover pre-midnight catch-up
  for (const dayOffset of [-1, 0]) {
    const date = new Date(Date.UTC(
      now.getUTCFullYear(),
      now.getUTCMonth(),
      now.getUTCDate() + dayOffset,
    ));

    // create the four fixed UTC cycle bases
    for (const hour of SCHEDULE_HOURS) {
      const base = new Date(Date.UTC(
        date.getUTCFullYear(),
        date.getUTCMonth(),
        date.getUTCDate(),
        hour,
        35,
      ));

      // exclude cycles at or beyond the finite boundary
      if (base.getTime() < Date.parse(ADJUSTMENT_EVIDENCE_CAPTURE_START) ||
        base.getTime() >= Date.parse(captureEndAt)) {
        continue;
      }
      const dueKey = `capture/${base.toISOString()}`;
      const jitterSeconds = Number.parseInt(sha256(`ballydidean|${dueKey}`).slice(0, 8), 16) % 300;
      candidates.push({
        dueKey,
        jitterSeconds,
        scheduledAt: new Date(base.getTime() + jitterSeconds * 1_000).toISOString(),
      });
    }
  }
  return candidates;
}

// identify skipped due keys since the last durable clock observation
function missedAdjustmentEvidenceDueKeys(lastCheckedAt, now, selectedDueKey, captureEndAt) {
  // treat first startup as one current catch-up without invented history
  if (lastCheckedAt === null) {
    return [];
  }

  const previous = Date.parse(lastCheckedAt);
  const candidates = [];

  // inspect each UTC date between bounded scheduler observations
  for (let cursor = new Date(previous); cursor.getTime() <= now.getTime();
    cursor = new Date(cursor.getTime() + 24 * 60 * 60 * 1_000)) {
    candidates.push(...adjustmentEvidenceDueCandidates(cursor, captureEndAt));
  }

  return [...new Map(candidates.map(
    // deduplicate overlapping yesterday/today candidate windows
    (candidate) => [candidate.dueKey, candidate],
  )).values()]
    .filter(
      // retain only elapsed, unselected cycle gaps after the last observation
      (candidate) => Date.parse(candidate.scheduledAt) > previous &&
        Date.parse(candidate.scheduledAt) <= now.getTime() &&
        candidate.dueKey !== selectedDueKey,
    )
    .sort((left, right) => left.scheduledAt.localeCompare(right.scheduledAt))
    .map(
      // project only the immutable due identity
      (candidate) => candidate.dueKey,
    );
}

// retain the complete finite scheduler history without pruning
function retainSchedulerHistory(entries) {
  // stop on an impossible overrun instead of deleting causal history
  if (entries.length > SCHEDULER_HISTORY_LIMIT) {
    throw new Error("scheduler history capacity is exhausted");
  }
  return entries;
}

// normalize one internal trigger result to a closed status label
function normalizeSchedulerTriggerStatus(result) {
  const status = result?.status;

  // accept only explicit successful capture outcomes
  if (status !== "created" && status !== "duplicate") {
    const error = new Error("scheduler capture was not committed");
    error.code = "capture_not_committed";
    throw error;
  }
  return status;
}

// reduce one internal failure to a bounded categorical code
function boundedErrorCode(error) {
  const value = typeof error?.code === "string" ? error.code : "capture_failed";
  return /^[a-z0-9_]{1,64}$/u.test(value) ? value : "capture_failed";
}

// require one trusted issuance-channel value
function requireIssuanceChannel(value) {
  if (!ISSUANCE_CHANNELS.includes(value)) {
    throw new TypeError("issuance channel is invalid");
  }
  return value;
}

// parse one canonical content-addressed channel assertion
function parseChannelAssertion(bytes, expectedSha256) {
  const value = JSON.parse(bytes.toString("utf8"));
  validateChannelAssertion(value, expectedSha256);

  // require exact canonical bytes at every reader boundary
  if (!bytes.equals(Buffer.from(`${canonicalJson(value)}\n`))) {
    throw new TypeError("channel assertion encoding is invalid");
  }
  return value;
}

// validate one content-addressed channel assertion
function validateChannelAssertion(value, expectedSha256) {
  requireExactKeys(value, [
    "channel",
    "contractVersion",
    "edgeReceiptIdentitySha256",
    "firstEdgeCommittedAt",
    "receiptSha256",
  ], "channel assertion");
  requireEqual(
    value.contractVersion,
    ADJUSTMENT_EVIDENCE_CHANNEL_CONTRACT_VERSION,
    "channel assertion contractVersion",
  );
  requireIssuanceChannel(value.channel);
  requireSha256(value.edgeReceiptIdentitySha256, "channel assertion identity");
  requireInstant(value.firstEdgeCommittedAt, "channel assertion firstEdgeCommittedAt");
  requireSha256(value.receiptSha256, "channel assertion receiptSha256");
  const bytes = Buffer.from(`${canonicalJson(value)}\n`);

  // bind the content-addressed filename to exact assertion bytes
  if (sha256(bytes) !== expectedSha256) {
    throw new TypeError("channel assertion hash is invalid");
  }
}

// parse one canonical stable receipt-to-assertion binding
function parseChannelBinding(bytes, expectedIdentity) {
  const value = JSON.parse(bytes.toString("utf8"));
  validateChannelBinding(value, expectedIdentity);

  // reject noncanonical mutable binding bytes
  if (!bytes.equals(Buffer.from(`${canonicalJson(value)}\n`))) {
    throw new TypeError("channel binding encoding is invalid");
  }
  return value;
}

// validate one stable receipt-to-assertion binding
function validateChannelBinding(value, expectedIdentity) {
  requireExactKeys(value, [
    "assertionSha256",
    "contractVersion",
    "edgeReceiptIdentitySha256",
  ], "channel binding");
  requireEqual(value.contractVersion, "issuance-channel-binding/v2", "channel binding contractVersion");
  requireEqual(value.edgeReceiptIdentitySha256, expectedIdentity, "channel binding identity");
  requireSha256(value.assertionSha256, "channel binding assertionSha256");
}

// map an immutable object contract to its matching receipt contract
function receiptVersionForObject(contractVersion) {
  // preserve the exact v1 pairing
  if (contractVersion === ADJUSTMENT_EVIDENCE_OBJECT_CONTRACT_VERSION) {
    return ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION;
  }

  // accept only the new v2 pairing otherwise
  if (contractVersion === ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION) {
    return ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION;
  }
  throw new TypeError("adjustment evidence object contract is invalid");
}

// require one exact scheduler due-key grammar
function requireSchedulerDueKey(value) {
  if (typeof value !== "string" ||
    !/^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u.test(value)) {
    throw new TypeError("scheduler dueKey is invalid");
  }
}

// read one optional private regular file without following links
async function readOptionalPrivateFile(path, maximumBytes) {
  return await readPrivateFileIfPresent(path, maximumBytes);
}

// read one optional private file and distinguish only absence
async function readPrivateFileIfPresent(path, maximumBytes) {
  try {
    return await readRegularFile(path, maximumBytes);
  } catch (error) {
    // treat only one absent path as empty state
    if (error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
}

// atomically replace one bounded private status file and fsync its directory
async function writeAtomicPrivate(path, bytes) {
  const directoryPath = dirname(path);
  const temporaryPath = join(directoryPath, `.state-${randomUUID()}.tmp`);
  const handle = await open(
    temporaryPath,
    fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW,
    0o600,
  );

  try {
    try {
      await handle.writeFile(bytes);
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporaryPath, path);
    await fsyncDirectory(directoryPath);
  } catch (error) {
    await unlink(temporaryPath).catch(() => undefined);
    throw error;
  }
}

// fsync one real directory after rename or slot reuse
async function fsyncDirectory(path) {
  const handle = await open(
    path,
    fsConstants.O_RDONLY | fsConstants.O_DIRECTORY | fsConstants.O_NOFOLLOW,
  );

  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

// create one classified projection limit error
function objectTooLargeError() {
  const error = new RangeError("adjustment evidence object is too large");
  error.code = "adjustment_evidence_object_too_large";
  return error;
}

// create canonical JSON with recursively sorted object keys
function canonicalJson(value) {
  return JSON.stringify(canonicalValue(value));
}

// encode one canonical newline-framed document
function canonicalJsonBytes(value) {
  return Buffer.from(`${canonicalJson(value)}\n`, "utf8");
}

// recreate the stable receipt identity from one stored object
function stableIdentitySha256(object) {
  return sha256(canonicalJson({
    bundleIdentities: object.bundleIdentities,
    rows: object.rows.map(
      // exclude all scoring values from the stable record identity
      (row) => ({ record: row.record, source: row.source }),
    ),
    settingsSha256: object.settingsSha256,
    siteKey: object.siteKey,
    window: object.window,
  }));
}

// recursively sort plain object keys and reject unsupported values
function canonicalValue(value) {
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }

  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new TypeError("canonical JSON numbers must be finite");
    }
    return value;
  }

  if (Array.isArray(value)) {
    return value.map(
      // preserve semantic array order
      (entry) => canonicalValue(entry),
    );
  }

  requirePlainObject(value, "canonical value");
  return Object.fromEntries(Object.keys(value).sort().map(
    // normalize every property recursively
    (key) => [key, canonicalValue(value[key])],
  ));
}

// calculate one lowercase SHA-256 digest
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// require one plain object
function requirePlainObject(value, path) {
  if (value === null || Array.isArray(value) || typeof value !== "object" ||
    Object.getPrototypeOf(value) !== Object.prototype) {
    throw new TypeError(`${path} must be a plain object`);
  }
  return value;
}

// require exact closed object keys
function requireExactKeys(value, keys, path) {
  requirePlainObject(value, path);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();

  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    throw new TypeError(`${path} has invalid keys`);
  }
}

// require one exact value
function requireEqual(value, expected, path) {
  if (value !== expected) {
    throw new TypeError(`${path} is invalid`);
  }
}

// require one SHA-256 value
function requireSha256(value, path) {
  if (typeof value !== "string" || !SHA256_PATTERN.test(value)) {
    throw new TypeError(`${path} is invalid`);
  }
  return value;
}

// normalize an unavailable SHA-256 identity
function nullableSha256(value) {
  if (value === null || value === undefined) {
    return null;
  }
  return requireSha256(value, "sha256");
}

// require one canonical UTC instant
function requireInstant(value, path) {
  if (typeof value !== "string" || !value.endsWith("Z") ||
    !Number.isFinite(Date.parse(value)) || new Date(value).toISOString() !== value) {
    throw new TypeError(`${path} is invalid`);
  }
}

// return one validated UTC instant
function instant(value, path) {
  requireInstant(value, path);
  return value;
}

// normalize an unavailable UTC instant
function nullableInstant(value, path) {
  if (value === null || value === undefined) {
    return null;
  }
  return instant(value, path);
}

// require one bounded opaque string
function requireBoundedString(value, path) {
  if (typeof value !== "string" || value.length < 1 || value.length > 200 ||
    /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new TypeError(`${path} is invalid`);
  }
  return value;
}

// normalize an unavailable bounded string
function nullableBoundedString(value, path) {
  if (value === null || value === undefined) {
    return null;
  }
  return requireBoundedString(value, path);
}

// require one bounded string array
function requireStringArray(value, path) {
  if (!Array.isArray(value) || value.length > 16) {
    throw new TypeError(`${path} is invalid`);
  }
  return value.map(
    // validate every metric name
    (entry) => requireBoundedString(entry, `${path}[]`),
  );
}

// require one nonnegative record revision
function requireRevisionCount(value) {
  if (!Number.isSafeInteger(value) || value < 0 || value > 1_000_000) {
    throw new TypeError("row.revisionCount is invalid");
  }
  return value;
}

// validate one closed authoritative database revision receipt
function validateRevisionCommitReceipt(value) {
  requireExactKeys(value, [
    "archiveCommitOrdinal", "archiveCommittedAt", "contractVersion", "frontierSha256",
    "predecessorFrontierSha256", "projectionIdentitySha256", "projectionKind",
    "projectionSha256", "receiptSha256", "stageReceiptSha256",
  ], "adjustment revision receipt");
  // require one selected serving or shadow receipt contract and server ordinal
  if (value.contractVersion !== ADJUSTMENT_REVISION_COMMIT_RECEIPT_CONTRACT_VERSION ||
    !["actual_best_match", "native_source", "rain_gate_input", "shadow_prediction",
      "target_revision"]
      .includes(value.projectionKind)) {
    throw new TypeError("adjustment revision receipt contract is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal, "adjustment revision receipt ordinal");
  requireInstant(value.archiveCommittedAt, "adjustment revision archive commit time");
  // reject the reserved genesis ordinal
  if (value.archiveCommitOrdinal === "0") {
    throw new TypeError("adjustment revision receipt ordinal is invalid");
  }
  for (const field of [
    "frontierSha256", "predecessorFrontierSha256", "projectionIdentitySha256",
    "projectionSha256", "receiptSha256", "stageReceiptSha256",
  ]) {
    requireSha256(value[field], `adjustment revision receipt ${field}`);
  }
  // keep canonical serving body hashes equal while preserving shadow's distinct grammar
  if (value.projectionKind !== "shadow_prediction" &&
    value.projectionSha256 !== value.projectionIdentitySha256) {
    throw new TypeError("adjustment revision receipt projection identity differs");
  }
  return value;
}

// recompute one server receipt and its claimed frontier identity
function validateRevisionCommitReceiptIdentity(value) {
  validateRevisionCommitReceipt(value);
  const receiptSha256 = sha256(Buffer.from([
    value.contractVersion,
    value.archiveCommitOrdinal,
    value.archiveCommittedAt,
    value.projectionKind,
    value.projectionIdentitySha256,
    value.projectionSha256,
    value.stageReceiptSha256,
    value.predecessorFrontierSha256,
  ].join("\n")));
  const frontierSha256 = sha256(Buffer.from([
    "adjustment-revision-frontier/v1",
    value.predecessorFrontierSha256,
    value.archiveCommitOrdinal,
    receiptSha256,
  ].join("\n")));
  // reject caller-hashed or internally inconsistent database receipts
  if (value.receiptSha256 !== receiptSha256 || value.frontierSha256 !== frontierSha256) {
    throw new Error("adjustment revision receipt identity differs");
  }
  return value;
}

// validate and decode one exact shadow stage transport request
function validateShadowStageInput(input) {
  const comparatorTransport = input !== null && typeof input === "object" &&
    !Array.isArray(input) && Object.hasOwn(input, "comparatorBase64");
  requireExactKeys(input, comparatorTransport ? [
    "bodyBase64", "comparatorBase64", "metadata", "sourceProjectionBase64",
    "sourceProjectionSha256",
  ] : [
    "bodyBase64", "metadata", "sourceProjectionBase64", "sourceProjectionSha256",
  ], "adjustment shadow revision stage input");
  const body = decodeCanonicalBase64(input.bodyBase64,
    "adjustment shadow revision body");
  const comparator = comparatorTransport
    ? decodeCanonicalBase64(input.comparatorBase64,
        "adjustment shadow incumbent comparator")
    : null;
  const source = decodeCanonicalBase64(input.sourceProjectionBase64,
    "adjustment shadow revision source");
  requireShadowMetadata(input.metadata);
  requireSha256(input.sourceProjectionSha256,
    "adjustment shadow revision source identity");
  // bind the compact metadata to exact private source and body bytes
  if (input.sourceProjectionSha256 !== sha256(source) ||
    input.metadata.predictionBodySha256 !== sha256(body) ||
    input.metadata.bodyByteCount !== body.length) {
    throw new Error("adjustment shadow revision stage bytes differ");
  }
  return {
    body,
    comparator,
    comparatorSha256: comparator === null ? null : sha256(comparator),
    input,
    metadata: input.metadata,
    source,
  };
}

// encode the producer-frozen receipt field order before hashing
function shadowReceiptIdentityBytes(value, version) {
  // retain v1 and additive v2 as two explicit closed receipt grammars
  if (!["v1", "v2"].includes(version) ||
    typeof value.contractVersion !== "string" ||
    !value.contractVersion.endsWith(`/${version}`)) {
    throw new TypeError("adjustment shadow receipt version is invalid");
  }
  return Buffer.from(`${JSON.stringify(value)}\n`);
}

// require the fixed value-free shadow database metadata shape
function requireShadowMetadata(value) {
  requireExactKeys(value, [
    "bodyByteCount", "candidateSha256", "dueKey", "inputSha256", "issuedAt",
    "maxValidAt", "minValidAt", "predictionBodySha256", "predictionSchemaSha256",
    "predictionSha256", "registrationSha256", "rowCount", "sourceReceiptSha256",
    "sourceSha256",
  ], "adjustment shadow revision metadata");
  // retain only bounded body geometry and the scheduled capture key
  if (!Number.isSafeInteger(value.bodyByteCount) || value.bodyByteCount < 2 ||
    value.bodyByteCount > ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES || !Number.isSafeInteger(value.rowCount) ||
    value.rowCount < 1 || value.rowCount > 168 ||
    typeof value.dueKey !== "string" ||
    !/^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u.test(value.dueKey)) {
    throw new TypeError("adjustment shadow revision metadata bounds are invalid");
  }
  for (const field of [
    "candidateSha256", "inputSha256", "predictionBodySha256",
    "predictionSchemaSha256", "predictionSha256", "registrationSha256",
    "sourceReceiptSha256", "sourceSha256",
  ]) {
    requireSha256(value[field], `adjustment shadow revision metadata ${field}`);
  }
  requireInstant(value.issuedAt, "adjustment shadow revision issued time");
  requireInstant(value.minValidAt, "adjustment shadow revision minimum valid time");
  requireInstant(value.maxValidAt, "adjustment shadow revision maximum valid time");
  return value;
}

// parse one canonical durable shadow stage document
function parseShadowRevisionStage(bytes) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, ["contractVersion", "input", "stageReceipt"],
    "adjustment shadow revision stage");
  // require one exact immutable stage representation
  const version = value.contractVersion === "adjustment-shadow-revision-stage/v2"
    ? "v2"
    : "v1";
  if (value.contractVersion !== `adjustment-shadow-revision-stage/${version}` ||
    !bytes.equals(canonicalJsonBytes(value))) {
    throw new Error("adjustment shadow revision stage is not canonical");
  }
  const prepared = validateShadowStageInput(value.input);
  const stage = value.stageReceipt;
  requireExactKeys(stage, version === "v2" ? [
    "contractVersion", "durable", "durableAt", "dueKey", "predictionBodySha256",
    "registrationSha256", "sourceProjectionSha256", "comparatorSha256",
    "stageReceiptSha256",
  ] : [
    "contractVersion", "durable", "durableAt", "dueKey", "predictionBodySha256",
    "registrationSha256", "sourceProjectionSha256", "stageReceiptSha256",
  ], "adjustment shadow stage receipt");
  requireInstant(stage.durableAt, "adjustment shadow stage durable time");
  requireSha256(stage.stageReceiptSha256, "adjustment shadow stage receipt identity");
  const unsigned = {
    contractVersion: stage.contractVersion,
    durable: stage.durable,
    durableAt: stage.durableAt,
    dueKey: stage.dueKey,
    predictionBodySha256: stage.predictionBodySha256,
    registrationSha256: stage.registrationSha256,
    sourceProjectionSha256: stage.sourceProjectionSha256,
    ...(version === "v2" ? { comparatorSha256: stage.comparatorSha256 } : {}),
  };
  // bind the durable receipt to the exact retained stage bytes
  if (stage.contractVersion !== `adjustment-shadow-stage-receipt/${version}` ||
    stage.durable !== true || stage.dueKey !== prepared.metadata.dueKey ||
    stage.predictionBodySha256 !== prepared.metadata.predictionBodySha256 ||
    stage.registrationSha256 !== prepared.metadata.registrationSha256 ||
    stage.sourceProjectionSha256 !== value.input.sourceProjectionSha256 ||
    (version === "v2" &&
      (prepared.comparatorSha256 === null ||
        stage.comparatorSha256 !== prepared.comparatorSha256)) ||
    (version === "v1" && prepared.comparator !== null) ||
    stage.stageReceiptSha256 !== sha256(shadowReceiptIdentityBytes(unsigned, version))) {
    throw new Error("adjustment shadow stage receipt differs");
  }
  return value;
}

// parse one canonical permanent four-kind gap document
function parseRevisionGap(bytes) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "contractVersion", "gapSha256", "input", "qualificationDisposition", "recordedAt",
  ], "adjustment revision gap");
  requireExactKeys(value.input, [
    "logicalKeySha256", "projectionIdentitySha256", "projectionKind",
    "projectionSha256", "reason",
  ], "adjustment revision gap input");
  requireSha256(value.input.logicalKeySha256, "adjustment revision gap logical key");
  requireInstant(value.recordedAt, "adjustment revision gap recorded time");
  const input = value.input;
  // require the same closed categorical and nullable body grammar as the writer
  if (value.contractVersion !== "adjustment-revision-gap/v1" ||
    value.qualificationDisposition !== "forever_unqualified" ||
    !["actual_best_match", "native_source", "rain_gate_input", "target_revision"]
      .includes(input.projectionKind) ||
    !["archive_stage_failed", "database_bind_failed", "database_admission_failed",
      "archive_publish_failed"].includes(input.reason) ||
    ((input.projectionSha256 === null) !== (input.projectionIdentitySha256 === null)) ||
    (input.projectionSha256 !== null &&
      (input.projectionSha256 !== input.projectionIdentitySha256 ||
        !/^[a-f0-9]{64}$/u.test(input.projectionSha256)))) {
    throw new TypeError("adjustment revision gap is invalid");
  }
  const unsigned = {
    contractVersion: value.contractVersion,
    input: value.input,
    qualificationDisposition: value.qualificationDisposition,
    recordedAt: value.recordedAt,
  };
  // bind the permanent identity and exact canonical bytes
  if (value.gapSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    !bytes.equals(canonicalJsonBytes(value))) {
    throw new Error("adjustment revision gap identity differs");
  }
  return Object.freeze(value);
}

// parse one canonical permanent shadow gap document
function parseShadowRevisionGap(bytes) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "contractVersion", "gapSha256", "input", "qualificationDisposition", "recordedAt",
  ], "adjustment shadow gap");
  requireExactKeys(value.input, ["dueKey", "family", "reason", "registrationSha256"],
    "adjustment shadow gap input");
  requireShadowMetadataGapInput(value.input);
  requireInstant(value.recordedAt, "adjustment shadow gap recorded time");
  const unsigned = {
    contractVersion: value.contractVersion,
    input: value.input,
    qualificationDisposition: value.qualificationDisposition,
    recordedAt: value.recordedAt,
  };
  // bind the permanent identity and exact canonical bytes
  if (value.contractVersion !== "adjustment-shadow-gap/v1" ||
    value.qualificationDisposition !== "forever_unqualified" ||
    value.gapSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    !bytes.equals(canonicalJsonBytes(value))) {
    throw new Error("adjustment shadow gap identity differs");
  }
  return Object.freeze(value);
}

// validate only the value-free scheduled shadow gap input
function requireShadowMetadataGapInput(input) {
  requireSha256(input.registrationSha256, "adjustment shadow gap registration");
  // retain the frozen schedule, family and failure category sets
  if (typeof input.dueKey !== "string" ||
    !/^capture\/\d{4}-\d{2}-\d{2}T(?:00|06|12|18):35:00\.000Z$/u.test(input.dueKey) ||
    !["rain", "temperature", "wind"].includes(input.family) ||
    !["candidate_unavailable", "comparator_unavailable", "source_incomplete", "archive_stage_failed",
      "database_append_failed", "database_admission_failed", "archive_publish_failed"]
      .includes(input.reason)) {
    throw new TypeError("adjustment shadow gap is invalid");
  }
}

// parse one stable shadow publication acknowledgement
function parseShadowRevisionPublishReceipt(bytes, predictionSha256,
  comparatorSha256 = null) {
  const value = JSON.parse(bytes.toString("utf8"));
  const version = comparatorSha256 === null ? "v1" : "v2";
  requireExactKeys(value, version === "v2" ? [
    "committed", "committedAt", "contractVersion", "predictionSha256",
    "comparatorSha256", "publishReceiptSha256",
  ] : [
    "committed", "committedAt", "contractVersion", "predictionSha256",
    "publishReceiptSha256",
  ], "adjustment shadow publish receipt");
  requireInstant(value.committedAt, "adjustment shadow publish commit time");
  const unsigned = {
    committed: value.committed,
    committedAt: value.committedAt,
    contractVersion: value.contractVersion,
    predictionSha256: value.predictionSha256,
    ...(version === "v2" ? { comparatorSha256: value.comparatorSha256 } : {}),
  };
  // bind the stored acknowledgement to its exact compact prediction
  if (!bytes.equals(canonicalJsonBytes(value)) || value.committed !== true ||
    value.contractVersion !== `adjustment-shadow-publish-receipt/${version}` ||
    value.predictionSha256 !== predictionSha256 ||
    (version === "v2" && value.comparatorSha256 !== comparatorSha256) ||
    value.publishReceiptSha256 !== sha256(shadowReceiptIdentityBytes(unsigned, version))) {
    throw new Error("adjustment shadow publish receipt differs");
  }
  return Object.freeze(value);
}

// parse one canonical shadow source/body capsule retained by its database receipt
export function parseShadowRevisionCapsule(bytes) {
  // reject oversized or noncanonical private payloads before graph publication
  if (!Buffer.isBuffer(bytes) || bytes.length < 2 || bytes.length > 2 * 1024 * 1024) {
    throw new RangeError("adjustment shadow revision capsule bytes are invalid");
  }
  const value = JSON.parse(bytes.toString("utf8"));
  const version = value?.contractVersion === "adjustment-shadow-revision-capsule/v2"
    ? "v2"
    : "v1";
  requireExactKeys(value, version === "v2" ? [
    "bodyBase64", "comparatorBase64", "comparatorSha256", "contractVersion",
    "metadata", "predictionCommittedAt", "revisionReceipt", "sourceProjectionBase64",
    "sourceProjectionSha256", "stageReceipt",
  ] : [
    "bodyBase64", "contractVersion", "metadata", "revisionReceipt",
    "sourceProjectionBase64", "sourceProjectionSha256", "stageReceipt",
  ], "adjustment shadow revision capsule");
  // preserve one unique canonical capsule encoding
  if (value.contractVersion !== `adjustment-shadow-revision-capsule/${version}` ||
    !bytes.equals(canonicalJsonBytes(value))) {
    throw new TypeError("adjustment shadow revision capsule is not canonical");
  }
  const stageInput = {
    bodyBase64: value.bodyBase64,
    ...(version === "v2" ? { comparatorBase64: value.comparatorBase64 } : {}),
    metadata: value.metadata,
    sourceProjectionBase64: value.sourceProjectionBase64,
    sourceProjectionSha256: value.sourceProjectionSha256,
  };
  const prepared = validateShadowStageInput(stageInput);
  const stageDocument = parseShadowRevisionStage(canonicalJsonBytes({
    contractVersion: `adjustment-shadow-revision-stage/${version}`,
    input: stageInput,
    stageReceipt: value.stageReceipt,
  }));
  validateRevisionCommitReceiptIdentity(value.revisionReceipt);
  // retain the database row clock independently of the later archive commit clock
  if (version === "v2") {
    requireInstant(value.predictionCommittedAt,
      "adjustment shadow prediction commit time");
  }
  // bind exact staged bytes, compact metadata and the distinct shadow receipt grammar
  if (value.revisionReceipt.projectionKind !== "shadow_prediction" ||
    value.revisionReceipt.projectionIdentitySha256 !==
      prepared.metadata.sourceReceiptSha256 ||
    value.revisionReceipt.projectionSha256 !== prepared.metadata.inputSha256 ||
    value.revisionReceipt.stageReceiptSha256 !==
      stageDocument.stageReceipt.stageReceiptSha256 ||
    (version === "v2" &&
      (Date.parse(value.predictionCommittedAt) < Date.parse(value.metadata.issuedAt) ||
        Date.parse(value.predictionCommittedAt) >
          Date.parse(value.revisionReceipt.archiveCommittedAt))) ||
    (version === "v2" && value.comparatorSha256 !== prepared.comparatorSha256)) {
    throw new Error("adjustment shadow revision capsule binding differs");
  }
  return value;
}

// decode one unique padded base64 field
function decodeCanonicalBase64(value, label) {
  // reject whitespace and alternate base64 spellings without recursive matching
  if (typeof value !== "string" || value.length % 4 !== 0) {
    throw new TypeError(`${label} base64 is invalid`);
  }
  const paddingLength = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  const content = paddingLength === 0 ? value : value.slice(0, -paddingLength);
  if (/[^A-Za-z0-9+/]/u.test(content) || content.includes("=") ||
      (paddingLength === 1 && content.length % 4 !== 3) ||
      (paddingLength === 2 && content.length % 4 !== 2)) {
    throw new TypeError(`${label} base64 is invalid`);
  }
  const bytes = Buffer.from(value, "base64");
  // require round-trip equality for one exact wire form
  if (bytes.toString("base64") !== value) {
    throw new TypeError(`${label} base64 differs`);
  }
  return bytes;
}

// parse one canonical durable-stage receipt and recompute its unsigned identity
function parseRevisionStageReceipt(bytes, projectionIdentitySha256, projectionKind,
  projectionSha256) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "contractVersion", "durable", "durableAt", "projectionIdentitySha256",
    "projectionKind", "projectionSha256", "stageReceiptSha256",
  ], "adjustment revision stage receipt");
  const unsigned = {
    contractVersion: value.contractVersion,
    durable: value.durable,
    durableAt: value.durableAt,
    projectionIdentitySha256: value.projectionIdentitySha256,
    projectionKind: value.projectionKind,
    projectionSha256: value.projectionSha256,
  };
  requireInstant(value.durableAt, "adjustment revision stage durable time");
  // bind the stored receipt to canonical bytes and the requested body
  if (!bytes.equals(canonicalJsonBytes(value)) ||
    value.contractVersion !== ADJUSTMENT_REVISION_STAGE_RECEIPT_CONTRACT_VERSION ||
    value.durable !== true || value.projectionIdentitySha256 !== projectionIdentitySha256 ||
    value.projectionKind !== projectionKind || value.projectionSha256 !== projectionSha256 ||
    value.stageReceiptSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment revision stage receipt differs");
  }
  return Object.freeze(value);
}

// parse one canonical rain-control state stage receipt
function parseRainControlStateStageReceipt(bytes, stateSha256) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "contractVersion", "durable", "durableAt", "stageReceiptSha256", "stateSha256",
  ], "adjustment rain control state stage receipt");
  const unsigned = {
    contractVersion: value.contractVersion,
    durable: value.durable,
    durableAt: value.durableAt,
    stateSha256: value.stateSha256,
  };
  requireInstant(value.durableAt, "adjustment rain control state durable time");
  // bind canonical receipt bytes to the state's producer identity
  if (!bytes.equals(canonicalJsonBytes(value)) ||
    value.contractVersion !== "adjustment-rain-control-state-stage-receipt/v1" ||
    value.durable !== true || value.stateSha256 !== stateSha256 ||
    value.stageReceiptSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment rain control state stage receipt differs");
  }
  return Object.freeze(value);
}

// parse one canonical permanent state abandonment and its exact producer input
function parseRainControlStateGap(bytes, expectedInput) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "contractVersion", "gapAt", "gapSha256", "reason", "stageReceiptSha256",
    "stateSha256",
  ], "adjustment rain control state gap");
  requireInstant(value.gapAt, "adjustment rain control state gap time");
  requireSha256(value.gapSha256, "adjustment rain control state gap identity");
  requireSha256(value.stageReceiptSha256,
    "adjustment rain control state gap stage receipt");
  requireSha256(value.stateSha256, "adjustment rain control state gap state");
  const unsigned = { ...value };
  delete unsigned.gapSha256;
  // bind canonical bytes and every frozen request field to the durable record
  if (!bytes.equals(canonicalJsonBytes(value)) ||
    value.contractVersion !== ADJUSTMENT_RAIN_CONTROL_STATE_GAP_CONTRACT_VERSION ||
    value.gapSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
    value.reason !== expectedInput.reason ||
    value.stageReceiptSha256 !== expectedInput.stageReceiptSha256 ||
    value.stateSha256 !== expectedInput.stateSha256) {
    throw new Error("adjustment rain control state gap differs");
  }
  return Object.freeze(value);
}

// validate one exact fixed-gauge target gap key or producer disposition
function validateRainFixedGaugeTargetGapKey(input, requireReason) {
  requireExactKeys(input, requireReason
    ? ["logicalHourAt", "logicalKeySha256", "reason"]
    : ["logicalHourAt", "logicalKeySha256"],
  "adjustment rain fixed-gauge target gap input");
  requireInstant(input.logicalHourAt, "adjustment rain fixed-gauge target hour");
  requireSha256(input.logicalKeySha256,
    "adjustment rain fixed-gauge target logical key");
  // admit only exact hour boundaries and the frozen pre-stage failure class
  if (Date.parse(input.logicalHourAt) % 3_600_000 !== 0 ||
      (requireReason && input.reason !== "target_source_oversized")) {
    throw new TypeError("adjustment rain fixed-gauge target gap input differs");
  }
}

// address one permanent gap without using a caller-supplied file name
function rainFixedGaugeTargetGapKeySha256(input) {
  return sha256(canonicalJsonBytes({
    logicalHourAt: input.logicalHourAt,
    logicalKeySha256: input.logicalKeySha256,
  }));
}

// parse one canonical value-free oversized target disposition
function parseRainFixedGaugeTargetGap(bytes, expectedInput) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "contractVersion", "gapAt", "gapSha256", "logicalHourAt",
    "logicalKeySha256", "qualificationDisposition", "reason",
  ], "adjustment rain fixed-gauge target gap");
  requireInstant(value.gapAt, "adjustment rain fixed-gauge target gap time");
  requireSha256(value.gapSha256, "adjustment rain fixed-gauge target gap identity");
  requireSha256(value.logicalKeySha256,
    "adjustment rain fixed-gauge target gap logical key");
  const unsigned = { ...value };
  delete unsigned.gapSha256;
  // bind the durable bytes to the exact lookup key and immutable disposition
  if (!bytes.equals(canonicalJsonBytes(value)) ||
      value.contractVersion !== ADJUSTMENT_RAIN_FIXED_GAUGE_TARGET_GAP_CONTRACT_VERSION ||
      value.gapSha256 !== sha256(canonicalJsonBytes(unsigned)) ||
      value.logicalHourAt !== expectedInput.logicalHourAt ||
      value.logicalKeySha256 !== expectedInput.logicalKeySha256 ||
      value.reason !== "target_source_oversized" ||
      value.qualificationDisposition !== "forever_unqualified") {
    throw new Error("adjustment rain fixed-gauge target gap differs");
  }
  return Object.freeze(value);
}

// parse one canonical durable-publication receipt and recompute its unsigned identity
function parseRevisionPublishReceipt(bytes, revision) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "committed", "committedAt", "contractVersion", "projectionIdentitySha256",
    "projectionKind", "projectionSha256", "publishReceiptSha256",
    "revisionReceiptSha256",
  ], "adjustment revision publish receipt");
  const unsigned = {
    committed: value.committed,
    committedAt: value.committedAt,
    contractVersion: value.contractVersion,
    projectionIdentitySha256: value.projectionIdentitySha256,
    projectionKind: value.projectionKind,
    projectionSha256: value.projectionSha256,
    revisionReceiptSha256: value.revisionReceiptSha256,
  };
  requireInstant(value.committedAt, "adjustment revision publish commit time");
  // bind the stored publication to canonical bytes and the database receipt
  if (!bytes.equals(canonicalJsonBytes(value)) || value.committed !== true ||
    value.contractVersion !== ADJUSTMENT_REVISION_PUBLISH_RECEIPT_CONTRACT_VERSION ||
    value.projectionIdentitySha256 !== revision.projectionIdentitySha256 ||
    value.projectionKind !== revision.projectionKind ||
    value.projectionSha256 !== revision.projectionSha256 ||
    value.revisionReceiptSha256 !== revision.receiptSha256 ||
    value.publishReceiptSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment revision publish receipt differs");
  }
  return Object.freeze(value);
}

// parse one canonical grouped publication bound to every ordered receipt
function parseRevisionBatchPublishReceipt(bytes, receipts) {
  const value = JSON.parse(bytes.toString("utf8"));
  requireExactKeys(value, [
    "committed", "committedAt", "contractVersion", "projectionIdentitySha256",
    "projectionKind", "projectionSha256", "publishReceiptSha256",
    "revisionReceiptSha256s",
  ], "adjustment revision batch publish receipt");
  const first = receipts[0];
  const receiptSha256s = receipts.map(
    // preserve the exact global-frontier order
    (receipt) => receipt.receiptSha256,
  );
  const unsigned = {
    committed: value.committed,
    committedAt: value.committedAt,
    contractVersion: value.contractVersion,
    projectionIdentitySha256: value.projectionIdentitySha256,
    projectionKind: value.projectionKind,
    projectionSha256: value.projectionSha256,
    revisionReceiptSha256s: value.revisionReceiptSha256s,
  };
  requireInstant(value.committedAt, "adjustment revision batch publish commit time");
  if (!Array.isArray(value.revisionReceiptSha256s) ||
    value.revisionReceiptSha256s.length !== receipts.length) {
    throw new TypeError("adjustment revision batch publish receipts are invalid");
  }
  for (const receiptSha256 of value.revisionReceiptSha256s) {
    requireSha256(receiptSha256, "adjustment revision batch publish receipt identity");
  }
  // bind canonical bytes, one staged body and the complete database receipt order
  if (!bytes.equals(canonicalJsonBytes(value)) || value.committed !== true ||
    value.contractVersion !== ADJUSTMENT_REVISION_BATCH_PUBLISH_RECEIPT_CONTRACT_VERSION ||
    value.projectionIdentitySha256 !== first.projectionIdentitySha256 ||
    value.projectionKind !== first.projectionKind ||
    value.projectionSha256 !== first.projectionSha256 ||
    canonicalJson(value.revisionReceiptSha256s) !== canonicalJson(receiptSha256s) ||
    value.publishReceiptSha256 !== sha256(canonicalJsonBytes(unsigned))) {
    throw new Error("adjustment revision batch publish receipt differs");
  }
  return Object.freeze(value);
}

// validate one complete grouped receipt and successor representation
function validateRevisionBatchCommitGroup(value) {
  requireExactKeys(value, [
    "contractVersion", "projectionIdentitySha256", "receipts", "successors",
  ], "adjustment revision batch commit group");
  requireSha256(value.projectionIdentitySha256,
    "adjustment revision batch group projection");
  if (value.contractVersion !== ADJUSTMENT_REVISION_BATCH_COMMIT_GROUP_CONTRACT_VERSION ||
    !Array.isArray(value.receipts) || !Array.isArray(value.successors) ||
    value.receipts.length < 1 ||
    value.receipts.length > ADJUSTMENT_REVISION_BATCH_MAXIMUM_RECEIPTS ||
    value.successors.length !== value.receipts.length) {
    throw new TypeError("adjustment revision batch commit group is invalid");
  }
  const first = value.receipts[0];
  // restrict grouped storage to one canonical staged weather body
  if (!["actual_best_match", "target_revision"].includes(first.projectionKind) ||
    first.projectionIdentitySha256 !== value.projectionIdentitySha256 ||
    first.projectionSha256 !== value.projectionIdentitySha256) {
    throw new Error("adjustment revision batch commit group projection differs");
  }
  let prior = null;
  // bind every embedded successor to one direct receipt in global order
  for (const [index, receipt] of value.receipts.entries()) {
    validateRevisionCommitReceiptIdentity(receipt);
    const successor = value.successors[index];
    requireExactKeys(successor, [
      "archiveCommitOrdinal", "contractVersion", "frontierSha256",
      "predecessorFrontierSha256", "receiptSha256",
    ], "adjustment revision batch successor");
    if (receipt.projectionIdentitySha256 !== value.projectionIdentitySha256 ||
      receipt.projectionSha256 !== first.projectionSha256 ||
      receipt.projectionKind !== first.projectionKind ||
      receipt.stageReceiptSha256 !== first.stageReceiptSha256 ||
      (prior !== null &&
        (receipt.predecessorFrontierSha256 !== prior.frontierSha256 ||
          BigInt(receipt.archiveCommitOrdinal) !== BigInt(prior.archiveCommitOrdinal) + 1n)) ||
      successor.archiveCommitOrdinal !== receipt.archiveCommitOrdinal ||
      successor.contractVersion !== "adjustment-revision-successor/v1" ||
      successor.frontierSha256 !== receipt.frontierSha256 ||
      successor.predecessorFrontierSha256 !== receipt.predecessorFrontierSha256 ||
      successor.receiptSha256 !== receipt.receiptSha256) {
      throw new Error("adjustment revision batch commit group differs");
    }
    prior = receipt;
  }
  return Object.freeze(value);
}

// normalize one injected archive clock to canonical UTC milliseconds
function requireNowInstant(value, label) {
  const instant = value instanceof Date ? value.toISOString() : value;
  requireInstant(instant, label);
  return instant;
}

// parse one canonical root-bound finalization proof
function parseMaintenanceFinalizationProof(bytesValue) {
  // require one bounded canonical proof document
  if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
    bytesValue.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new TypeError("maintenance finalization proof bytes are invalid");
  }
  let value;
  try {
    value = JSON.parse(bytesValue.toString("utf8"));
  } catch {
    throw new TypeError("maintenance finalization proof JSON is invalid");
  }
  // select the additive future-only proof without widening legacy v1 bytes
  if (value?.contractVersion === ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_V3_CONTRACT_VERSION) {
    validateAdjustmentMaintenanceFinalizationProofV3(value);
    if (!bytesValue.equals(canonicalJsonBytes(value))) {
      throw new TypeError("future-only maintenance finalization proof is not canonical");
    }
    return value;
  }
  requireExactKeys(value, [
    "actionSha256", "archiveCommitOrdinal", "burnSha256", "catalogGeneration",
    "catalogRevisionReceiptRootSha256", "catalogRootSha256",
    "catalogWatermarkSha256", "confirmationChunkCount", "contractVersion",
    "controlSha256", "controlVersion", "finalizedAt", "frontierCount",
    "frontierRootSha256", "fullMemberRootSha256", "graphManifestSha256",
    "inputSealSha256", "lifecycleLedgerRootSha256", "manifestSha256",
    "reportSha256", "requiredInputRootSha256", "retirementEntries",
    "retirementSetRootSha256", "revisionSnapshotRootSha256", "sourceCommit",
    "transferredAnchorSha256", "workstationJournalHeadSha256",
  ], "maintenance finalization proof");
  // require one controller-validated complete confirmation population
  if (value.contractVersion !== ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_CONTRACT_VERSION ||
    ![24, 27].includes(value.confirmationChunkCount) ||
    !/^[1-9][0-9]{0,5}$/u.test(value.controlVersion) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("maintenance finalization proof contract is invalid");
  }
  requireUint64Text(value.archiveCommitOrdinal, "finalization archiveCommitOrdinal");
  requireUint64Text(value.catalogGeneration, "finalization catalogGeneration");
  requireInstant(value.finalizedAt, "maintenance finalization finalizedAt");
  // require every root that crosses database, catalog, graph and controller state
  for (const field of [
    "actionSha256", "burnSha256", "catalogRevisionReceiptRootSha256",
    "catalogRootSha256", "catalogWatermarkSha256", "controlSha256",
    "frontierRootSha256", "fullMemberRootSha256", "graphManifestSha256",
    "inputSealSha256", "lifecycleLedgerRootSha256", "manifestSha256",
    "reportSha256", "requiredInputRootSha256", "retirementSetRootSha256",
    "revisionSnapshotRootSha256", "transferredAnchorSha256",
    "workstationJournalHeadSha256",
  ]) {
    requireSha256(value[field], `maintenance finalization ${field}`);
  }
  if (!Number.isSafeInteger(value.frontierCount) || value.frontierCount < 1 ||
    value.frontierCount > 9_500_000 || !Array.isArray(value.retirementEntries) ||
    value.retirementEntries.length < 5 || value.retirementEntries.length > 51) {
    throw new TypeError("maintenance finalization proof bounds are invalid");
  }
  const entries = value.retirementEntries.map(
    // validate each fixed safe retirement identity
    (entry) => validateMaintenanceRetirementEntry(entry),
  );
  // require canonical ordering and canonical proof bytes
  if (canonicalJson(entries) !== canonicalJson([...entries].sort(compareRetirementEntries)) ||
    !bytesValue.equals(canonicalJsonBytes(value))) {
    throw new TypeError("maintenance finalization proof is not canonical");
  }
  return value;
}

// validate one exact online metadata retirement entry
function validateMaintenanceRetirementEntry(value) {
  requireExactKeys(value, [
    "coldMemberSha256", "fileSha256", "identitySha256", "kind",
  ], "maintenance retirement entry");
  // permit only metadata made redundant by the verified cold graph
  if (!["archive_acknowledgement", "cycle_acknowledgement", "final_acknowledgement",
    "final_manifest", "input_seal", "revision_commit_receipt",
    "revision_frontier_successor", "revision_projection",
    "revision_publish_receipt", "revision_stage_receipt", "revision_terminal_gap",
    "shadow_revision_capsule", "shadow_revision_publish_receipt",
    "shadow_revision_terminal_gap"].includes(value.kind)) {
    throw new TypeError("maintenance retirement entry kind is invalid");
  }
  for (const field of ["coldMemberSha256", "fileSha256", "identitySha256"]) {
    requireSha256(value[field], `maintenance retirement entry ${field}`);
  }
  // require the cold member to retain the exact retired bytes
  if (value.coldMemberSha256 !== value.fileSha256) {
    throw new Error("maintenance retirement cold member differs");
  }
  return value;
}

// compare the fixed retirement entry order
function compareRetirementEntries(left, right) {
  return left.kind.localeCompare(right.kind, "en") ||
    left.identitySha256.localeCompare(right.identitySha256, "en");
}

// bind one finalization proof to every installed anchor identity
function validateFinalizationProofAgainstAnchor(proof, anchor, transferredSha256) {
  const bindings = {
    actionSha256: anchor.actionSha256,
    catalogGeneration: anchor.catalogGeneration,
    catalogRootSha256: anchor.catalogRootSha256,
    catalogWatermarkSha256: anchor.catalogWatermarkSha256,
    controlSha256: anchor.controlSha256,
    controlVersion: anchor.controlVersion,
    frontierCount: anchor.frontierCount,
    frontierRootSha256: anchor.frontierRootSha256,
    fullMemberRootSha256: anchor.fullMemberRootSha256,
    graphManifestSha256: anchor.graphManifestSha256,
    lifecycleLedgerRootSha256: anchor.lifecycleLedgerRootSha256,
    manifestSha256: anchor.manifestSha256,
    reportSha256: anchor.reportSha256,
    revisionSnapshotRootSha256: anchor.revisionSnapshotRootSha256,
    sourceCommit: anchor.sourceCommit,
    transferredAnchorSha256: transferredSha256,
    workstationJournalHeadSha256: anchor.workstationJournalHeadSha256,
  };
  // require all action roots to exist before F or burn authorization
  if (Object.values(bindings).some((value) => value === null) ||
    Object.entries(bindings).some(([key, value]) => proof[key] !== value) ||
    BigInt(proof.archiveCommitOrdinal) < BigInt(proof.frontierCount) ||
    Date.parse(proof.finalizedAt) < Date.parse(anchor.publishedAt)) {
    throw new Error("maintenance finalization anchor binding differs");
  }
}

// collect every exact cycle metadata file made redundant by one cold graph
async function collectMaintenanceRetirementEntries(root, ports, final) {
  const entries = [];
  // capture one exact redundant metadata representation
  const add = async (kind, identitySha256, path, maximumBytes) => {
    const bytes = await readRegularFile(path, maximumBytes);
    const fileSha256 = sha256(bytes);
    entries.push({ coldMemberSha256: fileSha256, fileSha256, identitySha256, kind });
  };
  // bind both server and workstation acknowledgements for every page
  for (const page of final.manifest.pages) {
    const acknowledgement = await ports.readPageAcknowledgement(page.pageSha256);
    const transfer = await ports.readTransferAcknowledgement(page.pageSha256);
    if (acknowledgement === null || transfer === null) {
      throw new Error("maintenance retirement acknowledgement is unavailable");
    }
    await add("cycle_acknowledgement", acknowledgement.acknowledgementSha256,
      join(root, "online-pages", "acknowledgements",
        `sha256-${acknowledgement.acknowledgementSha256}.json`), 2_048);
    await add("archive_acknowledgement", page.pageSha256,
      join(root, "online-pages", "archive-acknowledgements",
        `sha256-${page.pageSha256}.json`), 2_048);
  }
  const sealIdentity = sha256(canonicalJsonBytes({
    dueKey: final.manifest.dueKey,
    generation: final.manifest.generation,
  }));
  await add("input_seal", sealIdentity,
    join(root, "online-pages", "input-seals", `sha256-${sealIdentity}.json`), 2_048);
  await add("final_manifest", final.manifestSha256,
    join(root, "online-pages", "final-manifests",
      `sha256-${final.manifestSha256}.json`), 32 * 1_024);
  await add("final_acknowledgement", final.manifestSha256,
    join(root, "online-pages", "final-acknowledgements",
      `sha256-${final.manifestSha256}.json`), 2_048);
  const receiptRoot = join(root, "revision-commit-receipts");
  let receiptNames = [];
  try {
    receiptNames = (await readdir(receiptRoot)).sort();
  } catch (error) {
    // accept a legacy cycle with no revision spool
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  // retire no more than the two reviewed online payload slots per finalization
  if (receiptNames.length > ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS ||
    receiptNames.some((name) => !/^sha256-[a-f0-9]{64}\.json$/u.test(name))) {
    throw new Error("maintenance revision retirement population differs");
  }
  // bind every successful online slot and all provenance metadata it made redundant
  for (const name of receiptNames) {
    const receiptPath = join(receiptRoot, name);
    const receiptBytes = await readRegularFile(receiptPath, 4096);
    const receipt = validateRevisionCommitReceiptIdentity(
      JSON.parse(receiptBytes.toString("utf8")),
    );
    // require the content-addressed frontier name before adding cold redundancy
    if (name !== `sha256-${receipt.frontierSha256}.json` ||
      !receiptBytes.equals(canonicalJsonBytes(receipt))) {
      throw new Error("maintenance revision retirement receipt differs");
    }
    await add("revision_commit_receipt", receipt.frontierSha256,
      receiptPath, 4096);
    await add("revision_frontier_successor", receipt.predecessorFrontierSha256,
      join(root, "revision-frontier-successors",
        `sha256-${receipt.predecessorFrontierSha256}.json`), 4096);
    // select exact serving or shadow file paths without scanning unrelated roots
    if (receipt.projectionKind === "shadow_prediction") {
      const capsulePath = join(root, "shadow-revision-capsules",
        `sha256-${receipt.receiptSha256}.json`);
      const capsule = parseShadowRevisionCapsule(
        await readRegularFile(capsulePath, 2 * 1024 * 1024),
      );
      await add("shadow_revision_capsule", receipt.receiptSha256,
        capsulePath, 2 * 1024 * 1024);
      const publishPath = join(root, "shadow-revision-publish-receipts",
        `sha256-${capsule.metadata.predictionSha256}.json`);
      const publishBytes = await readOptionalPrivateFile(publishPath, 4096);
      // retire either the exact publication or its permanent terminal gap
      if (publishBytes !== null) {
        parseShadowRevisionPublishReceipt(
          publishBytes,
          capsule.metadata.predictionSha256,
          capsule.comparatorSha256 ?? null,
        );
        await add("shadow_revision_publish_receipt", capsule.metadata.predictionSha256,
          publishPath, 4096);
      } else {
        const gaps = (await readStoredShadowRevisionGaps(root)).filter(
          // address only this admitted capsule's failed publication
          (gap) => gap.input.dueKey === capsule.metadata.dueKey &&
            gap.input.registrationSha256 === capsule.metadata.registrationSha256 &&
            gap.input.reason === "archive_publish_failed",
        );
        // require one actual terminal member before retirement
        if (gaps.length !== 1) {
          throw new Error("maintenance shadow terminal gap differs");
        }
        const gapIdentitySha256 = sha256(canonicalJsonBytes({
          dueKey: gaps[0].input.dueKey,
          registrationSha256: gaps[0].input.registrationSha256,
        }));
        await add("shadow_revision_terminal_gap", gapIdentitySha256,
          join(root, "shadow-revision-gaps", `sha256-${gapIdentitySha256}.json`), 4096);
      }
    } else {
      await add("revision_projection", receipt.projectionIdentitySha256,
        join(root, "revision-projections",
          `sha256-${receipt.projectionIdentitySha256}.json`), ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES);
      await add("revision_stage_receipt", receipt.projectionIdentitySha256,
        join(root, "revision-stage-receipts",
          `sha256-${receipt.projectionIdentitySha256}.json`), 4096);
      const publishPath = join(root, "revision-publish-receipts",
        `sha256-${receipt.projectionIdentitySha256}.json`);
      const publishBytes = await readOptionalPrivateFile(publishPath, 4096);
      // retire either the exact publication or its permanent terminal gap
      if (publishBytes !== null) {
        parseRevisionPublishReceipt(publishBytes, receipt);
        await add("revision_publish_receipt", receipt.projectionIdentitySha256,
          publishPath, 4096);
      } else {
        const gaps = (await readStoredRevisionGaps(root)).filter(
          // address only this admitted body's failed publication
          (gap) => gap.input.projectionIdentitySha256 ===
            receipt.projectionIdentitySha256 &&
            gap.input.projectionKind === receipt.projectionKind &&
            gap.input.reason === "archive_publish_failed",
        );
        // require one actual terminal member before retirement
        if (gaps.length !== 1) {
          throw new Error("maintenance revision terminal gap differs");
        }
        const gapIdentitySha256 = sha256(canonicalJsonBytes({
          logicalKeySha256: gaps[0].input.logicalKeySha256,
          projectionIdentitySha256: gaps[0].input.projectionIdentitySha256,
        }));
        await add("revision_terminal_gap", gapIdentitySha256,
          join(root, "revision-gaps", `sha256-${gapIdentitySha256}.json`), 4096);
      }
    }
  }
  return entries.sort(compareRetirementEntries);
}

// map only fixed retirement identities to existing private paths
function maintenanceRetirementPath(root, entry) {
  const revisionDirectories = {
    revision_commit_receipt: "revision-commit-receipts",
    revision_frontier_successor: "revision-frontier-successors",
    revision_projection: "revision-projections",
    revision_publish_receipt: "revision-publish-receipts",
    revision_stage_receipt: "revision-stage-receipts",
    revision_terminal_gap: "revision-gaps",
    shadow_revision_capsule: "shadow-revision-capsules",
    shadow_revision_publish_receipt: "shadow-revision-publish-receipts",
    shadow_revision_terminal_gap: "shadow-revision-gaps",
  };
  // map revision spool members directly under the existing private evidence root
  if (Object.hasOwn(revisionDirectories, entry.kind)) {
    return join(root, revisionDirectories[entry.kind],
      `sha256-${entry.identitySha256}.json`);
  }
  const directories = {
    archive_acknowledgement: "archive-acknowledgements",
    cycle_acknowledgement: "acknowledgements",
    final_acknowledgement: "final-acknowledgements",
    final_manifest: "final-manifests",
    input_seal: "input-seals",
  };
  return join(root, "online-pages", directories[entry.kind],
    `sha256-${entry.identitySha256}.json`);
}

// retire only exact hash-bound files after F is durable
async function retireMaintenanceHotFiles(root, entries) {
  let retiredFiles = 0;
  const directories = new Set();
  // verify each remaining redundant representation immediately before unlink
  for (const entry of entries) {
    validateMaintenanceRetirementEntry(entry);
    const path = maintenanceRetirementPath(root, entry);
    let bytes;
    try {
      const maximumBytes = entry.kind === "final_manifest"
        ? 32 * 1_024
        : entry.kind === "revision_projection"
          ? 768 * 1_024
          : entry.kind === "shadow_revision_capsule"
            ? 2 * 1024 * 1024
            : 4096;
      bytes = await readRegularFile(path, maximumBytes);
    } catch (error) {
      // accept only an exact retry after prior retirement
      if (error?.code === "ENOENT") {
        continue;
      }
      throw error;
    }
    if (sha256(bytes) !== entry.fileSha256) {
      throw new Error("maintenance retirement file differs");
    }
    await unlink(path);
    directories.add(dirname(path));
    retiredFiles += 1;
  }
  // make every exact unlink durable before reporting retirement
  for (const directory of directories) {
    await fsyncDirectory(directory);
  }
  return retiredFiles;
}

// parse one canonical value-free maintenance anchor
function parseMaintenanceAnchor(bytesValue) {
  // require one bounded buffer with canonical newline framing
  if (!Buffer.isBuffer(bytesValue) || bytesValue.length < 2 ||
    bytesValue.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new TypeError("maintenance anchor bytes are invalid");
  }
  let value;

  // decode only one complete JSON document
  try {
    value = JSON.parse(bytesValue.toString("utf8"));
  } catch {
    throw new TypeError("maintenance anchor JSON is invalid");
  }
  // select the additive future-only anchor without relabelling legacy v2 bytes
  if (value?.contractVersion === ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION) {
    validateAdjustmentMaintenanceAnchorV3(value);
    if (!bytesValue.equals(canonicalJsonBytes(value))) {
      throw new TypeError("future-only maintenance anchor is not canonical");
    }
    return value;
  }
  const baseKeys = [
    "actionSha256",
    "archiveObjectSha256",
    "catalogGeneration",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "contractVersion",
    "controlSha256",
    "controlVersion",
    "ctfState",
    "dueKey",
    "frontierCount",
    "frontierRootSha256",
    "fullGraphVerifiedAt",
    "fullMemberRootSha256",
    "generation",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "predecessorAnchorSha256",
    "publishedAt",
    "reportSha256",
    "revisionSnapshotRootSha256",
    "sequence",
    "sourceCommit",
    "workstationJournalHeadSha256",
  ];
  const finalizedKeys = [
    "archiveCommitOrdinal", "burnSha256", "catalogRevisionReceiptRootSha256",
    "confirmationChunkCount", "finalizationProofSha256", "finalizedAt",
    "inputSealSha256", "requiredInputRootSha256", "retirementSetRootSha256",
  ];
  requireExactKeys(value, value?.ctfState === "finalized"
    ? [...baseKeys, ...finalizedKeys] : baseKeys, "maintenance anchor");

  // permit only the two root-bound transfer and finalization states
  if (value.contractVersion !== ADJUSTMENT_MAINTENANCE_ANCHOR_CONTRACT_VERSION ||
    !["transferred", "finalized"].includes(value.ctfState) ||
    !/^[1-9][0-9]{0,5}$/u.test(value.controlVersion) ||
    !/^[a-f0-9]{40}$/u.test(value.sourceCommit)) {
    throw new TypeError("maintenance anchor contract is invalid");
  }
  requireSchedulerDueKey(value.dueKey);
  requireUint64Text(value.sequence, "maintenance anchor sequence");
  requireUint64Text(value.generation, "maintenance anchor generation");
  requireUint64Text(value.catalogGeneration, "maintenance anchor catalogGeneration");
  requireInstant(value.fullGraphVerifiedAt, "maintenance anchor fullGraphVerifiedAt");
  requireInstant(value.publishedAt, "maintenance anchor publishedAt");

  // require every T root that binds the verified cold graph and journal
  for (const field of [
    "archiveObjectSha256",
    "catalogRootSha256",
    "catalogWatermarkSha256",
    "controlSha256",
    "frontierRootSha256",
    "graphManifestSha256",
    "lifecycleLedgerRootSha256",
    "manifestSha256",
    "workstationJournalHeadSha256",
  ]) {
    requireSha256(value[field], `maintenance anchor ${field}`);
  }

  // validate every additional F and burn crossbinding
  if (value.ctfState === "finalized") {
    requireUint64Text(value.archiveCommitOrdinal, "maintenance anchor archiveCommitOrdinal");
    requireInstant(value.finalizedAt, "maintenance anchor finalizedAt");
    for (const field of [
      "burnSha256", "catalogRevisionReceiptRootSha256", "finalizationProofSha256",
      "inputSealSha256", "requiredInputRootSha256", "retirementSetRootSha256",
    ]) {
      requireSha256(value[field], `maintenance anchor ${field}`);
    }
    if (![24, 27].includes(value.confirmationChunkCount) ||
      value.actionSha256 === null || value.fullMemberRootSha256 === null ||
      value.reportSha256 === null || value.revisionSnapshotRootSha256 === null ||
      Date.parse(value.finalizedAt) < Date.parse(value.publishedAt)) {
      throw new TypeError("maintenance finalized anchor fields are invalid");
    }
  }

  // retain only explicit optional action projections
  for (const field of [
    "actionSha256",
    "fullMemberRootSha256",
    "predecessorAnchorSha256",
    "reportSha256",
    "revisionSnapshotRootSha256",
  ]) {
    if (value[field] !== null) {
      requireSha256(value[field], `maintenance anchor ${field}`);
    }
  }

  // keep the bounded frontier and causal clock order exact
  if (!Number.isSafeInteger(value.frontierCount) || value.frontierCount < 1 ||
    value.frontierCount > 9_500_000 ||
    Date.parse(value.fullGraphVerifiedAt) > Date.parse(value.publishedAt) ||
    !bytesValue.equals(Buffer.from(`${canonicalJson(value)}\n`))) {
    throw new TypeError("maintenance anchor fields are invalid");
  }
  return value;
}

// require one canonical uint64 decimal string
function requireUint64Text(value, path) {
  // reject alternate integer encodings and overflow
  if (typeof value !== "string" || !/^(?:0|[1-9][0-9]*)$/u.test(value) ||
    BigInt(value) > 0xffff_ffff_ffff_ffffn) {
    throw new TypeError(`${path} is invalid`);
  }
}

// project the anchor slots without action or recovery authority
function maintenanceAnchorStatus(privacyState, rootState, slotsValue) {
  const absent = { sha256: null, sizeBytes: null, state: "absent" };
  const current = slotsValue.current ?? absent;
  return Object.freeze({
    actionEligible: false,
    contractVersion: "adjustment-maintenance-anchor-status/v2",
    privacyState,
    rootState,
    schemaReadiness: rootState === "verified"
      ? current.state === "finalized"
        ? "finalized_anchor_installed"
        : "transferred_anchor_only"
      : "not_established",
    slots: {
      current,
      pending: slotsValue.pending ?? absent,
      previous: slotsValue.previous ?? absent,
    },
  });
}

// durably replace one fixed private slot without following links
async function writePrivateAtomic(path, bytes) {
  const directory = dirname(path);
  const temporary = join(directory, `.anchor-${randomUUID()}.tmp`);
  let handle = await open(
    temporary,
    fsConstants.O_CREAT | fsConstants.O_EXCL | fsConstants.O_WRONLY | fsConstants.O_NOFOLLOW,
    0o600,
  );

  try {
    await handle.writeFile(bytes);
    await handle.sync();
    await handle.close();
    handle = null;
    await rename(temporary, path);
    await fsyncDirectory(directory);
  } catch (error) {
    // close only the still-open transaction file
    if (handle !== null) {
      await handle.close().catch(() => undefined);
    }
    // remove only this transaction's unique temporary path
    try {
      await unlink(temporary);
    } catch (cleanupError) {
      if (cleanupError?.code !== "ENOENT") {
        error.maintenanceAnchorCleanupFailed = true;
      }
    }
    throw error;
  }
}

// require one bounded integer
function requireBoundedInteger(value, path) {
  if (!Number.isSafeInteger(value) || Math.abs(value) > 1_000_000) {
    throw new TypeError(`${path} is invalid`);
  }
  return value;
}

// require one finite number
function finite(value, path) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new TypeError(`${path} is invalid`);
  }
  return value;
}

// normalize one unavailable number
function nullableFinite(value, path) {
  if (value === null || value === undefined) {
    return null;
  }
  return finite(value, path);
}

// require one adjustment decision object
function requireDecision(value, path) {
  return requirePlainObject(value, path);
}

// require the exact public state union for this family
function requireState(value, path, states) {
  // reject unknown and cross-family states
  if (!states.includes(value)) {
    throw new TypeError(`${path} is invalid`);
  }
  return value;
}

// derive the intended target release from root-owned state and public exact CI
async function resolveAdjustmentRevisionCaptureEpochDeployment(targetRelease, options) {
  // reject noncanonical immutable target release names
  if (!/^\d{4}\.\d{2}\.\d{2}-[1-9][0-9]?$/u.test(targetRelease)) {
    throw new TypeError("revision capture epoch target release is invalid");
  }
  const deployRoot = options.deployRoot ??
    resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const environmentPath = join(deployRoot, "releases", `${targetRelease}.env`);
  const bytes = await readRootOwnedReleaseEnvironment(environmentPath);
  const environment = parseAdjustmentRevisionCaptureEpochEnvironment(bytes);
  // bind the fixed environment to the requested immutable target
  if (environment.WEATHER_RELEASE !== targetRelease) {
    throw new Error("revision capture epoch target environment differs");
  }
  const verifier = options.verifyInertRelease ?? (await import(
    pathToFileURL(join(deployRoot, "scripts", "adjustment-evaluation-package.mjs")).href
  )).verifyAdjustmentInertV14GitRelease;
  const proof = await verifier(targetRelease);
  // accept only the independently verified exact target commit
  if (proof.targetRelease !== targetRelease ||
    !/^[a-f0-9]{40}$/u.test(proof.targetCommit)) {
    throw new Error("revision capture epoch Git proof differs");
  }
  return validateAdjustmentRevisionCaptureEpochDeployment({
    controlPlaneSha256: environment.WEATHER_CONTROL_PLANE_SHA256,
    controlPlaneVersion: environment.WEATHER_CONTROL_PLANE_VERSION,
    sourceCommit: proof.targetCommit,
    sourceRelease: targetRelease,
    sourceServerImageDigest: environment.WEATHER_SERVER_IMAGE.split("@")[1],
    sourceWebImageDigest: environment.WEATHER_WEB_IMAGE.split("@")[1],
  });
}

// read one fixed root-owned private release environment without links
async function readRootOwnedReleaseEnvironment(path) {
  const handle = await open(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  try {
    const details = await handle.stat();
    // require the same root-private single-link release-state boundary
    if (!details.isFile() || details.uid !== 0 || details.gid !== 0 ||
      details.nlink !== 1 || (details.mode & 0o777) !== 0o600 || details.size > 16 * 1024) {
      throw new Error("revision capture epoch target environment is invalid");
    }
    return await handle.readFile();
  } finally {
    await handle.close();
  }
}

// parse only the reviewed eleven-field v14 release environment
function parseAdjustmentRevisionCaptureEpochEnvironment(bytes) {
  const keys = [
    "CLOUDFLARED_IMAGE", "POSTGRES_IMAGE", "WEATHER_CONTROL_PLANE_SHA256",
    "WEATHER_CONTROL_PLANE_VERSION", "WEATHER_DATABASE_NAME",
    "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
    "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH", "WEATHER_POSTGRES_DIR",
    "WEATHER_RELEASE", "WEATHER_SERVER_IMAGE", "WEATHER_WEB_IMAGE",
  ];
  const lines = bytes.toString("utf8").split("\n");
  // require one trailing newline and exactly eleven declarations
  if (lines.at(-1) !== "" || lines.length !== keys.length + 1) {
    throw new Error("revision capture epoch target environment differs");
  }
  const entries = lines.slice(0, -1).map((line) => {
    const separator = line.indexOf("=");
    // reject empty, duplicate-shaped or nondeclarative lines
    if (separator < 1) {
      throw new Error("revision capture epoch target environment differs");
    }
    return [line.slice(0, separator), line.slice(separator + 1)];
  });
  const environment = Object.fromEntries(entries);
  // require the exact field population and fixed v14 image grammar
  if (entries.length !== Object.keys(environment).length ||
    canonicalJson(Object.keys(environment).sort()) !== canonicalJson(keys) ||
    environment.WEATHER_CONTROL_PLANE_VERSION !== "14" ||
    !/^[a-f0-9]{64}$/u.test(environment.WEATHER_CONTROL_PLANE_SHA256) ||
    !/^ghcr\.io\/anstosa\/weather-server@sha256:[a-f0-9]{64}$/u
      .test(environment.WEATHER_SERVER_IMAGE) ||
    !/^ghcr\.io\/anstosa\/weather-web@sha256:[a-f0-9]{64}$/u
      .test(environment.WEATHER_WEB_IMAGE)) {
    throw new Error("revision capture epoch target environment differs");
  }
  return environment;
}

// run closed archive verbs against only the installed fixed evidence root
export async function runAdjustmentArchiveCommand(argv, options = {}) {
  const [action, ...argumentsList] = argv;
  const revisionRoot = options.root ?? process.env.WEATHER_ADJUSTMENT_EVIDENCE_ROOT ??
    ADJUSTMENT_EVIDENCE_DEFAULT_ROOT;

  // create one root-owned future-only epoch before target release activation
  if (action === "revision-capture-epoch-init-v1") {
    // keep initialization root-only with one immutable target operand
    if (argumentsList.length !== 1 || process.geteuid() !== 0) {
      throw new TypeError("revision-capture-epoch-init-v1 requires root and TARGET_RELEASE");
    }
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedInput(options.stdin ?? process.stdin, 64 * 1024,
        "revision capture epoch database input");
    const databaseEnvelope = JSON.parse(bytes.toString("utf8"));
    validateAdjustmentRevisionCaptureEpochDatabaseEnvelope(databaseEnvelope);
    const deployment = options.captureEpochDeployment ??
      await resolveAdjustmentRevisionCaptureEpochDeployment(argumentsList[0], options);
    validateAdjustmentRevisionCaptureEpochDeployment(deployment);
    await initializeAdjustmentRevisionColdArchiveRoot({
      expectedGid: 10002,
      expectedUid: 10002,
      root: revisionRoot,
    });
    const witness = await writeAdjustmentRevisionCaptureEpochWitness({
      databaseEnvelope,
      deployment,
      path: options.captureEpochPath ?? ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH,
    });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(witness));
    return;
  }

  // export only the authenticated create-once future-only epoch
  if (action === "revision-capture-epoch-read-v1") {
    // reject operands on the fixed witness read
    if (argumentsList.length !== 0) {
      throw new TypeError("revision-capture-epoch-read-v1 takes no arguments");
    }
    const witness = await readAdjustmentRevisionCaptureEpochWitness({
      expectedGid: options.captureEpochExpectedGid ?? 0,
      expectedUid: options.captureEpochExpectedUid ?? 0,
      path: options.captureEpochPath ?? ADJUSTMENT_REVISION_CAPTURE_EPOCH_PATH,
    });
    const snapshot = await readAdjustmentRevisionCaptureEpochSnapshot({
      expectedGid: options.captureEpochExpectedGid ?? 0,
      expectedUid: options.captureEpochExpectedUid ?? 0,
      path: options.captureEpochSnapshotPath ??
        ADJUSTMENT_REVISION_CAPTURE_EPOCH_SNAPSHOT_PATH,
    });
    // expose no witness detached from its retained actual zero snapshot
    if (snapshot.snapshotSha256 !== witness.servingSnapshotSha256 ||
      snapshot.cutoffAt !== witness.epochAt) {
      throw new Error("revision capture epoch snapshot differs from witness");
    }
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(witness));
    return;
  }

  // export the actual canonical zero snapshot as its own cold graph member
  if (action === "revision-capture-epoch-snapshot-read-v1") {
    // reject operands on the fixed snapshot read
    if (argumentsList.length !== 0) {
      throw new TypeError("revision-capture-epoch-snapshot-read-v1 takes no arguments");
    }
    const snapshot = await readAdjustmentRevisionCaptureEpochSnapshot({
      expectedGid: options.captureEpochExpectedGid ?? 0,
      expectedUid: options.captureEpochExpectedUid ?? 0,
      path: options.captureEpochSnapshotPath ??
        ADJUSTMENT_REVISION_CAPTURE_EPOCH_SNAPSHOT_PATH,
    });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(snapshot));
    return;
  }

  // report only validated fixed-slot anchor projections
  if (action === "maintenance-anchor-status-v2") {
    if (argumentsList.length !== 0) {
      throw new TypeError("maintenance-anchor-status-v2 takes no arguments");
    }
    const status = await new AdjustmentMaintenanceAnchorStore({ root: options.root }).status();
    (options.stdout ?? process.stdout).write(Buffer.from(`${canonicalJson(status)}\n`));
    return;
  }

  // report only the current compact-metadata proof and durable preparation
  if (action === "shadow-metadata-custody-status-v1") {
    if (argumentsList.length !== 0) {
      throw new TypeError("shadow-metadata-custody-status-v1 takes no arguments");
    }
    const status = await new AdjustmentShadowMetadataCustodyProofStore({
      root: options.root,
    }).readStatus();
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(status));
    return;
  }

  // report only the last durable consumption for one exact proof
  if (action === "shadow-metadata-custody-consumed-v1") {
    if (argumentsList.length !== 1) {
      throw new TypeError(
        "shadow-metadata-custody-consumed-v1 requires PROOF_SHA256",
      );
    }
    requireSha256(argumentsList[0], "adjustment shadow metadata consumption proof");
    const consumption = await new AdjustmentShadowMetadataCustodyProofStore({
      root: options.root,
    }).readConsumed(argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(consumption));
    return;
  }

  // prepare exact owner arguments before the database transaction
  if (action === "shadow-metadata-custody-prepare-v1") {
    if (argumentsList.length !== 1) {
      throw new TypeError(
        "shadow-metadata-custody-prepare-v1 requires PROOF_SHA256",
      );
    }
    requireSha256(argumentsList[0], "adjustment shadow metadata custody proof");
    const input = await readCanonicalCommandInput(options,
      "adjustment shadow metadata preparation");
    requireExactKeys(input, ["finalizations", "proofSha256"],
      "adjustment shadow metadata preparation input");
    if (input.proofSha256 !== argumentsList[0]) {
      throw new Error("adjustment shadow metadata preparation proof differs");
    }
    const preparation = await new AdjustmentShadowMetadataCustodyProofStore({
      now: options.now,
      root: options.root,
    }).prepareFinalizations(input);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(preparation));
    return;
  }

  // consume only exact owner results bound to one durable preparation
  if (action === "shadow-metadata-custody-consume-v1") {
    if (argumentsList.length !== 1) {
      throw new TypeError(
        "shadow-metadata-custody-consume-v1 requires PREPARATION_SHA256",
      );
    }
    requireSha256(argumentsList[0],
      "adjustment shadow metadata custody preparation");
    const input = await readCanonicalCommandInput(options,
      "adjustment shadow metadata consumption");
    requireExactKeys(input, ["preparationSha256", "proofSha256", "results"],
      "adjustment shadow metadata custody consumption");
    if (input.preparationSha256 !== argumentsList[0]) {
      throw new Error("adjustment shadow metadata consumption preparation differs");
    }
    const consumption = await new AdjustmentShadowMetadataCustodyProofStore({
      now: options.now,
      root: options.root,
    }).consume(input);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(consumption));
    return;
  }

  // install one shadow-only development custody anchor
  if (action === "development-custody-anchor-install-v1") {
    if (argumentsList.length !== 1) {
      throw new TypeError(
        "development-custody-anchor-install-v1 requires ANCHOR_SHA256",
      );
    }
    requireSha256(argumentsList[0], "adjustment development custody anchor SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    const result = await new AdjustmentDevelopmentCustodyAnchorStore({
      captureEpochPath: options.captureEpochPath,
      expectedControlSha256: options.expectedControlSha256,
      expectedControlVersion: options.expectedControlVersion,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      root: options.root,
    }).install(bytes, argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(result));
    return;
  }

  // expose only the current server-derived development custody predecessor
  if (action === "development-custody-anchor-current-v1") {
    if (argumentsList.length !== 0) {
      throw new TypeError("development-custody-anchor-current-v1 takes no arguments");
    }
    const current = await new AdjustmentDevelopmentCustodyAnchorStore({
      root: options.root,
    }).readCurrent();
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(current));
    return;
  }

  // install one custody-only rain control-reference anchor
  if (action === "rain-control-custody-anchor-install-v1") {
    if (argumentsList.length !== 1) {
      throw new TypeError(
        "rain-control-custody-anchor-install-v1 requires ANCHOR_SHA256",
      );
    }
    requireSha256(argumentsList[0], "adjustment rain control custody anchor SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    const result = await new AdjustmentRainControlCustodyAnchorStore({
      captureEpochPath: options.captureEpochPath,
      expectedControlSha256: options.expectedControlSha256,
      expectedControlVersion: options.expectedControlVersion,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      root: options.root,
    }).install(bytes, argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(result));
    return;
  }

  // expose only the current server-derived rain control predecessor
  if (action === "rain-control-custody-anchor-current-v1") {
    if (argumentsList.length !== 0) {
      throw new TypeError("rain-control-custody-anchor-current-v1 takes no arguments");
    }
    const current = await new AdjustmentRainControlCustodyAnchorStore({
      root: options.root,
    }).readCurrent();
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(current));
    return;
  }

  // install one custody-only permanent unsupported terminal proof
  if (action === "unsupported-terminal-proof-install-v1") {
    if (argumentsList.length !== 1) {
      throw new TypeError(
        "unsupported-terminal-proof-install-v1 requires PROOF_SHA256",
      );
    }
    requireSha256(argumentsList[0], "adjustment unsupported terminal proof SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    const result = await new AdjustmentUnsupportedTerminalProofStore({
      captureEpochPath: options.captureEpochPath,
      expectedControlSha256: options.expectedControlSha256,
      expectedControlVersion: options.expectedControlVersion,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      readCurrentFamily: options.readCurrentFamily,
      root: options.root,
    }).install(bytes, argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(result));
    return;
  }

  // expose only the current server-derived unsupported predecessor
  if (action === "unsupported-terminal-proof-current-v1") {
    if (argumentsList.length !== 0) {
      throw new TypeError("unsupported-terminal-proof-current-v1 takes no arguments");
    }
    const current = await new AdjustmentUnsupportedTerminalProofStore({
      root: options.root,
    }).readCurrent();
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(current));
    return;
  }

  // expose only the exact current verified future-only input seal
  if (action === "future-input-seal-current-v2") {
    if (argumentsList.length !== 0) {
      throw new TypeError("future-input-seal-current-v2 takes no arguments");
    }
    const current = await new AdjustmentFutureOnlyInputSealStore({
      captureEpochPath: options.captureEpochPath,
      expectedControlSha256: options.expectedControlSha256,
      expectedControlVersion: options.expectedControlVersion,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      readSourceClosure: options.readSourceClosure,
      root: options.root,
    }).readCurrentVerified();
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(current));
    return;
  }

  // expose only the exact current verified future-only v3 anchor
  if (action === "maintenance-anchor-current-v3") {
    if (argumentsList.length !== 0) {
      throw new TypeError("maintenance-anchor-current-v3 takes no arguments");
    }
    const current = await new AdjustmentMaintenanceAnchorStore({
      captureEpochPath: options.captureEpochPath,
      expectedControlSha256: options.expectedControlSha256,
      expectedControlVersion: options.expectedControlVersion,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      readSourceClosure: options.readSourceClosure,
      root: options.root,
    }).readCurrentV3();
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(current));
    return;
  }

  // install one exact future-only seal after retained packed custody
  if (action === "future-input-seal-install-v2") {
    if (argumentsList.length !== 1) {
      throw new TypeError("future-input-seal-install-v2 requires SEAL_SHA256");
    }
    requireSha256(argumentsList[0], "future-only input seal SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    const result = await new AdjustmentFutureOnlyInputSealStore({
      captureEpochPath: options.captureEpochPath,
      readCaptureEpochWitness: options.readCaptureEpochWitness,
      root: options.root,
    }).install(bytes, argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(result));
    return;
  }

  // install only one hash-bound transferred anchor from bounded standard input
  if (action === "maintenance-anchor-install-v2") {
    if (argumentsList.length !== 1) {
      throw new TypeError("maintenance-anchor-install-v2 requires ANCHOR_SHA256");
    }
    requireSha256(argumentsList[0], "maintenance anchor SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    // keep the legacy forced verb closed to the frozen v2 anchor grammar
    if (parseMaintenanceAnchor(bytes).contractVersion !==
      ADJUSTMENT_MAINTENANCE_ANCHOR_CONTRACT_VERSION) {
      throw new TypeError("maintenance-anchor-install-v2 requires a v2 anchor");
    }
    await new AdjustmentMaintenanceAnchorStore({ root: options.root }).installTransferred(
      bytes,
      argumentsList[0],
    );
    return;
  }

  // install one future-only T anchor from a bounded canonical seal binding
  if (action === "maintenance-anchor-install-v3") {
    if (argumentsList.length !== 1) {
      throw new TypeError("maintenance-anchor-install-v3 requires ANCHOR_SHA256");
    }
    requireSha256(argumentsList[0], "future-only maintenance anchor SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    // keep the additive verb closed to only the future-only grammar
    if (parseMaintenanceAnchor(bytes).contractVersion !==
      ADJUSTMENT_MAINTENANCE_ANCHOR_V3_CONTRACT_VERSION) {
      throw new TypeError("maintenance-anchor-install-v3 requires a v3 anchor");
    }
    const result = await new AdjustmentMaintenanceAnchorStore({
      expectedControlSha256: options.expectedControlSha256,
      expectedControlVersion: options.expectedControlVersion,
      root: options.root,
    }).installTransferred(bytes, argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(result));
    return;
  }

  // finalize one installed T anchor from one bounded canonical cold proof
  if (action === "maintenance-anchor-finalize-v2") {
    if (argumentsList.length !== 1) {
      throw new TypeError("maintenance-anchor-finalize-v2 requires PROOF_SHA256");
    }
    requireSha256(argumentsList[0], "maintenance finalization proof SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    // keep the legacy forced verb closed to the frozen v1 proof grammar
    if (parseMaintenanceFinalizationProof(bytes).contractVersion !==
      ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_CONTRACT_VERSION) {
      throw new TypeError("maintenance-anchor-finalize-v2 requires a v1 proof");
    }
    await new AdjustmentMaintenanceAnchorStore({ root: options.root }).finalizeRetirement(
      bytes,
      argumentsList[0],
    );
    return;
  }

  // finalize one future-only T anchor without granting hot-file retirement
  if (action === "maintenance-anchor-finalize-v3") {
    if (argumentsList.length !== 1) {
      throw new TypeError("maintenance-anchor-finalize-v3 requires PROOF_SHA256");
    }
    requireSha256(argumentsList[0], "future-only finalization proof SHA256");
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedAnchorInput(options.stdin ?? process.stdin);
    // keep the additive verb closed to only the future-only proof grammar
    if (parseMaintenanceFinalizationProof(bytes).contractVersion !==
      ADJUSTMENT_MAINTENANCE_FINALIZATION_PROOF_V3_CONTRACT_VERSION) {
      throw new TypeError("maintenance-anchor-finalize-v3 requires a v3 proof");
    }
    const result = await new AdjustmentMaintenanceAnchorStore({
      root: options.root,
    }).finalizeRetirement(bytes, argumentsList[0]);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(result));
    return;
  }

  // emit one exact database-frozen revision transfer start
  if (action === "revision-cold-start-v1") {
    if (argumentsList.length !== 0) {
      throw new TypeError("revision-cold-start-v1 takes no arguments");
    }
    const bytes = Buffer.isBuffer(options.stdinBytes)
      ? Buffer.from(options.stdinBytes)
      : await readBoundedInput(options.stdin ?? process.stdin, 5 * 1024 * 1024,
        "revision cold start input");
    const envelope = JSON.parse(bytes.toString("utf8"));
    requireExactKeys(envelope, ["databaseManifest", "payload", "transaction"],
      "revision cold start database envelope");
    requirePlainObject(envelope.databaseManifest, "revision cold start database manifest");
    requireExactKeys(envelope.transaction, [
      "created_at_utc", "idle_in_transaction_session_timeout", "isolation_level",
      "lock_timeout", "read_only", "statement_timeout",
    ], "revision cold start transaction");
    // require the exact read-only repeatable-read transaction boundary
    if (envelope.transaction.isolation_level !== "repeatable read" ||
      envelope.transaction.read_only !== "on" ||
      envelope.transaction.idle_in_transaction_session_timeout !== "30s" ||
      envelope.transaction.lock_timeout !== "5s" ||
      envelope.transaction.statement_timeout !== "5min") {
      throw new Error("revision cold start transaction differs");
    }
    const start = await readAdjustmentRevisionColdTransferStart({
      root: revisionRoot,
      servingSnapshot: envelope.payload,
    });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(start));
    return;
  }

  // emit one two-slot revision successor page from fixed cursor operands
  if (action === "revision-cold-page-v1") {
    if (argumentsList.length !== 6) {
      throw new TypeError("revision-cold-page-v1 requires WATERMARK_ORDINAL WATERMARK_FRONTIER START_SHA256 AFTER_ORDINAL AFTER_FRONTIER PREVIOUS_PAGE_SHA256");
    }
    const [watermarkArchiveCommitOrdinal, watermarkFrontierSha256, startSha256,
      afterArchiveCommitOrdinal, afterFrontierSha256, previousPageSha256] = argumentsList;
    const page = await readAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal,
      afterFrontierSha256,
      previousPageSha256,
      root: revisionRoot,
      startSha256,
      watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256,
    });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(page));
    return;
  }

  // acknowledge one archived successful page and retire only its exact hot files
  if (action === "revision-cold-custody-ack-v1") {
    if (argumentsList.length !== 10) {
      throw new TypeError("revision-cold-custody-ack-v1 requires WATERMARK_ORDINAL WATERMARK_FRONTIER START_SHA256 AFTER_ORDINAL AFTER_FRONTIER PREVIOUS_PAGE_SHA256 PAGE_SHA256 GRAPH_SHA256 MEMBER_ROOT_SHA256 START_MEMBER_SHA256_OR_NONE");
    }
    const [watermarkArchiveCommitOrdinal, watermarkFrontierSha256, startSha256,
      afterArchiveCommitOrdinal, afterFrontierSha256, previousPageSha256,
      pageSha256, graphManifestSha256, memberRootSha256,
      startMemberSha256Value] = argumentsList;
    const startMemberSha256 = startMemberSha256Value === "none"
      ? null : startMemberSha256Value;
    const acknowledgement = await acknowledgeAdjustmentRevisionColdPage({
      afterArchiveCommitOrdinal,
      afterFrontierSha256,
      graphManifestSha256,
      memberRootSha256,
      pageSha256,
      previousPageSha256,
      root: revisionRoot,
      startMemberSha256,
      startSha256,
      watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256,
    }, options.revisionCustodyOptions);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(acknowledgement));
    return;
  }

  // acknowledge one locally packed page and retire only its exact hot files
  if (action === "revision-cold-custody-ack-v2") {
    // require the fixed cursor and checkpoint operand count
    if (argumentsList.length !== 10) {
      throw new TypeError("revision-cold-custody-ack-v2 requires WATERMARK_ORDINAL WATERMARK_FRONTIER START_SHA256 AFTER_ORDINAL AFTER_FRONTIER PREVIOUS_PAGE_SHA256 PAGE_SHA256 CUSTODY_CHECKPOINT_SHA256 MEMBER_ROOT_SHA256 START_MEMBER_SHA256_OR_NONE");
    }
    const [watermarkArchiveCommitOrdinal, watermarkFrontierSha256, startSha256,
      afterArchiveCommitOrdinal, afterFrontierSha256, previousPageSha256,
      pageSha256, custodyCheckpointSha256, memberRootSha256,
      startMemberSha256Value] = argumentsList;
    const startMemberSha256 = startMemberSha256Value === "none"
      ? null : startMemberSha256Value;
    const acknowledgement = await acknowledgeAdjustmentRevisionColdCustodyCheckpoint({
      afterArchiveCommitOrdinal,
      afterFrontierSha256,
      custodyCheckpointSha256,
      memberRootSha256,
      pageSha256,
      previousPageSha256,
      root: revisionRoot,
      startMemberSha256,
      startSha256,
      watermarkArchiveCommitOrdinal,
      watermarkFrontierSha256,
    }, options.revisionCustodyOptions);
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(acknowledgement));
    return;
  }

  // emit the current non-ordinal permanent-gap transfer frontier
  if (action === "revision-gap-start-v1") {
    if (argumentsList.length !== 0) {
      throw new TypeError("revision-gap-start-v1 takes no arguments");
    }
    const start = await readAdjustmentRevisionGapTransferStart({ root: revisionRoot });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(start));
    return;
  }

  // emit one frozen page of actual unqualified staged bytes
  if (action === "revision-gap-page-v1") {
    if (argumentsList.length !== 2) {
      throw new TypeError("revision-gap-page-v1 requires START_SHA256 FRONTIER_SHA256");
    }
    const [startSha256, frontierSha256] = argumentsList;
    const page = await readAdjustmentRevisionGapPayloadPage({
      frontierSha256,
      root: revisionRoot,
      startSha256,
    });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(page));
    return;
  }

  // acknowledge one exact verified cold graph and release its staged slots
  if (action === "revision-gap-ack-v1") {
    if (argumentsList.length !== 2) {
      throw new TypeError("revision-gap-ack-v1 requires PAGE_SHA256 GRAPH_SHA256");
    }
    const [pageSha256, graphManifestSha256] = argumentsList;
    const acknowledgement = await acknowledgeAdjustmentRevisionGapPayload({
      graphManifestSha256,
      pageSha256,
      root: revisionRoot,
    });
    (options.stdout ?? process.stdout).write(canonicalJsonBytes(acknowledgement));
    return;
  }

  // emit one bounded canonical page or idle envelope
  if (action === "archive-next") {
    if (argumentsList.length !== 0) {
      throw new TypeError("archive-next takes no arguments");
    }
    const transport = await new AdjustmentEvidenceArchiveTransport({
      now: options.now,
      root: options.root,
      statfs: options.statfs,
    }).initialize();
    const bytes = Buffer.from(`${canonicalJson(await transport.next())}\n`);

    // retain the forced-command response ceiling at the final byte boundary
    if (bytes.byteLength > ARCHIVE_TRANSFER_MAXIMUM_BYTES) {
      throw new Error("archive transfer envelope is too large");
    }
    (options.stdout ?? process.stdout).write(bytes);
    return;
  }

  // acknowledge only one exact page and open-cycle checkpoint pair
  if (action === "archive-ack") {
    if (argumentsList.length !== 2) {
      throw new TypeError("archive-ack requires PAGE_SHA256 CHECKPOINT_SHA256");
    }
    requireSha256(argumentsList[0], "pageSha256");
    requireSha256(argumentsList[1], "checkpointSha256");
    const transport = await new AdjustmentEvidenceArchiveTransport({
      now: options.now,
      root: options.root,
      statfs: options.statfs,
    }).initialize();
    await transport.acknowledge(argumentsList[0], argumentsList[1]);
    return;
  }

  // acknowledge only one genuine final manifest and cold graph pair
  if (action === "archive-ack-final") {
    if (argumentsList.length !== 2) {
      throw new TypeError("archive-ack-final requires MANIFEST_SHA256 GRAPH_SHA256");
    }
    requireSha256(argumentsList[0], "manifestSha256");
    requireSha256(argumentsList[1], "graphSha256");
    const transport = await new AdjustmentEvidenceArchiveTransport({
      now: options.now,
      root: options.root,
      statfs: options.statfs,
    }).initialize();
    await transport.acknowledgeFinal(argumentsList[0], argumentsList[1]);
    return;
  }
  throw new TypeError("archive operation is invalid");
}

// read at most one complete maintenance anchor from standard input
async function readBoundedAnchorInput(input) {
  const chunks = [];
  let length = 0;

  // bound every chunk before retaining it
  for await (const chunkValue of input) {
    const chunk = Buffer.from(chunkValue);
    length += chunk.length;

    // refuse input beyond one closed slot before further allocation
    if (length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
      throw new RangeError("maintenance anchor input is too large");
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks, length);
}

// read one generic bounded helper input without retaining overflow bytes
async function readBoundedInput(input, maximumBytes, label) {
  const chunks = [];
  let length = 0;
  // bound every input chunk before retaining it
  for await (const chunkValue of input) {
    const chunk = Buffer.from(chunkValue);
    length += chunk.length;
    // refuse overflow before appending the hostile chunk
    if (length > maximumBytes) {
      throw new RangeError(`${label} is too large`);
    }
    chunks.push(chunk);
  }
  // reject missing JSON input explicitly
  if (length < 2) {
    throw new RangeError(`${label} is empty`);
  }
  return Buffer.concat(chunks, length);
}

// parse one canonical bounded JSON helper request
async function readCanonicalCommandInput(options, label) {
  const bytes = Buffer.isBuffer(options.stdinBytes)
    ? Buffer.from(options.stdinBytes)
    : await readBoundedInput(options.stdin ?? process.stdin,
      ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES, label);
  if (bytes.length < 2 || bytes.length > ADJUSTMENT_MAINTENANCE_ANCHOR_MAXIMUM_BYTES) {
    throw new RangeError(`${label} is invalid`);
  }
  let value;
  try {
    value = JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new TypeError(`${label} JSON is invalid`);
  }
  // prohibit alternate whitespace, order or newline encodings
  if (!bytes.equals(canonicalJsonBytes(value))) {
    throw new TypeError(`${label} is not canonical`);
  }
  return value;
}

// recognize direct helper execution without affecting web-server imports
function isDirectAdjustmentArchiveCommand() {
  return typeof process.argv[1] === "string" &&
    import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
}

// execute only a direct closed helper command
if (isDirectAdjustmentArchiveCommand()) {
  runAdjustmentArchiveCommand(process.argv.slice(2)).catch(
    // return one bounded diagnostic and a failing forced-command status
    (error) => {
      process.stderr.write(`error: ${String(error?.message ?? "archive operation failed")}\n`);
      process.exitCode = 1;
    },
  );
}
