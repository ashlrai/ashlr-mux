export type BrowserImportScope = "cookiesAndHistory" | "everything";

export type BrowserImportMode =
  | "singleDestination"
  | "separateProfiles"
  | "mergeIntoOne";

export interface BrowserImportSourceProfile {
  browser_id: string;
  browser_name: string;
  profile_name: string;
  profile_path: string;
  importable_items: string[];
}

export interface BrowserImportDestinationProfile {
  id: string;
  display_name: string;
  is_default?: boolean;
}

export interface BrowserImportExecutionEntry {
  source_profile_paths: string[];
  source_profile_names: string[];
  destination_kind: "create" | "existing";
  destination_name: string;
  destination_profile_id?: string;
}

export interface BrowserImportStartRequest {
  browser_id: string;
  browser_name: string;
  mode: BrowserImportMode;
  scope: BrowserImportScope;
  entries: BrowserImportExecutionEntry[];
}

export interface BrowserImportSeparateDestinationChoice {
  kind: "create" | "existing";
  destinationProfileId?: string;
}

export function browserImportScopeFromSelection(selection: {
  cookies: boolean;
  history: boolean;
  additionalData: boolean;
}): BrowserImportScope {
  return selection.additionalData ? "everything" : "cookiesAndHistory";
}

export function defaultBrowserImportMode(
  selectedProfiles: readonly BrowserImportSourceProfile[],
): BrowserImportMode {
  return selectedProfiles.length > 1 ? "separateProfiles" : "singleDestination";
}

export function buildBrowserImportStartRequest(
  selectedProfiles: readonly BrowserImportSourceProfile[],
  options: {
    mode?: BrowserImportMode;
    scope: BrowserImportScope;
    destinationProfiles?: readonly BrowserImportDestinationProfile[];
    mergeDestinationProfileId?: string;
    separateDestinationChoices?: Readonly<
      Record<string, BrowserImportSeparateDestinationChoice>
    >;
  },
): BrowserImportStartRequest | null {
  if (selectedProfiles.length === 0) {
    return null;
  }
  const mode = options.mode ?? defaultBrowserImportMode(selectedProfiles);
  const browserId = selectedProfiles[0].browser_id;
  const browserName = selectedProfiles[0].browser_name;
  const destinationProfiles = normalizedDestinationProfiles(options.destinationProfiles);
  const mergeDestination =
    destinationProfiles.find(
      (profile) => profile.id === options.mergeDestinationProfileId,
    ) ??
    destinationProfiles.find((profile) => profile.is_default === true) ??
    destinationProfiles[0];

  return {
    browser_id: browserId,
    browser_name: browserName,
    mode,
    scope: options.scope,
    entries:
      mode === "mergeIntoOne"
        ? [
            {
              source_profile_paths: selectedProfiles.map((profile) => profile.profile_path),
              source_profile_names: selectedProfiles.map((profile) => profile.profile_name),
              destination_kind: "existing",
              destination_name: mergeDestination.display_name,
              destination_profile_id: mergeDestination.id,
            },
          ]
        : selectedProfiles.map((profile, index) => {
            const destinationName = uniqueDestinationName(
              profile.profile_name,
              selectedProfiles.slice(0, index).map((candidate) => candidate.profile_name),
            );
            const separateChoice =
              mode === "separateProfiles"
                ? options.separateDestinationChoices?.[profile.profile_path]
                : undefined;
            const separateDestination =
              separateChoice?.kind === "existing"
                ? destinationProfiles.find(
                    (candidate) => candidate.id === separateChoice.destinationProfileId,
                  )
                : undefined;
            return {
              source_profile_paths: [profile.profile_path],
              source_profile_names: [profile.profile_name],
              destination_kind:
                mode === "singleDestination" || separateDestination != null
                  ? "existing"
                  : "create",
              destination_name:
                mode === "singleDestination"
                  ? mergeDestination.display_name
                  : separateDestination?.display_name ?? destinationName,
              destination_profile_id:
                mode === "singleDestination"
                  ? mergeDestination.id
                  : separateDestination?.id,
            };
          }),
  };
}

export function defaultDestinationProfiles(): BrowserImportDestinationProfile[] {
  return [{ id: "default", display_name: "Default", is_default: true }];
}

export function normalizedDestinationProfiles(
  profiles?: readonly BrowserImportDestinationProfile[] | null,
): BrowserImportDestinationProfile[] {
  const sanitized =
    profiles
      ?.filter(
        (profile) =>
          profile.id.trim() !== "" && profile.display_name.trim() !== "",
      )
      .map((profile) => ({
        ...profile,
        id: profile.id.trim(),
        display_name: profile.display_name.trim(),
      })) ?? [];
  return sanitized.length > 0 ? sanitized : defaultDestinationProfiles();
}

function uniqueDestinationName(
  sourceName: string,
  previousSourceNames: readonly string[],
): string {
  const trimmed = sourceName.trim() || "Imported Profile";
  const duplicateIndex =
    previousSourceNames.filter((candidate) => candidate.trim() === trimmed).length + 1;
  return duplicateIndex === 1 ? trimmed : `${trimmed} (${duplicateIndex})`;
}
