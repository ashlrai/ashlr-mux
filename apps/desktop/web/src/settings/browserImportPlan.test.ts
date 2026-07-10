import { describe, expect, test } from "bun:test";

import {
  buildBrowserImportStartRequest,
  browserImportScopeFromSelection,
  defaultBrowserImportMode,
  normalizedDestinationProfiles,
  type BrowserImportSourceProfile,
} from "./browserImportPlan";

function profile(name: string, path = `C:/browser/${name}`): BrowserImportSourceProfile {
  return {
    browser_id: "chrome",
    browser_name: "Google Chrome",
    profile_name: name,
    profile_path: path,
    importable_items: ["bookmarks", "history", "cookies"],
  };
}

describe("browserImportPlan", () => {
  test("defaults to separate destination profiles for multiple source profiles", () => {
    const profiles = [profile("You"), profile("austin")];

    expect(defaultBrowserImportMode(profiles)).toBe("separateProfiles");
    expect(
      buildBrowserImportStartRequest(profiles, {
        scope: "cookiesAndHistory",
      }),
    ).toEqual({
      browser_id: "chrome",
      browser_name: "Google Chrome",
      mode: "separateProfiles",
      scope: "cookiesAndHistory",
      entries: [
        {
          source_profile_paths: ["C:/browser/You"],
          source_profile_names: ["You"],
          destination_kind: "create",
          destination_name: "You",
          destination_profile_id: undefined,
        },
        {
          source_profile_paths: ["C:/browser/austin"],
          source_profile_names: ["austin"],
          destination_kind: "create",
          destination_name: "austin",
          destination_profile_id: undefined,
        },
      ],
    });
  });

  test("merge mode captures a single existing destination", () => {
    expect(
      buildBrowserImportStartRequest([profile("You"), profile("austin")], {
        mode: "mergeIntoOne",
        scope: "cookiesAndHistory",
        destinationProfiles: [
          { id: "default", display_name: "Default", is_default: true },
          { id: "work", display_name: "Work" },
        ],
        mergeDestinationProfileId: "work",
      }),
    ).toEqual({
      browser_id: "chrome",
      browser_name: "Google Chrome",
      mode: "mergeIntoOne",
      scope: "cookiesAndHistory",
      entries: [
        {
          source_profile_paths: ["C:/browser/You", "C:/browser/austin"],
          source_profile_names: ["You", "austin"],
          destination_kind: "existing",
          destination_name: "Work",
          destination_profile_id: "work",
        },
      ],
    });
  });

  test("single profile defaults to the default existing destination", () => {
    const request = buildBrowserImportStartRequest([profile("You")], {
      scope: "cookiesAndHistory",
    });

    expect(defaultBrowserImportMode([profile("You")])).toBe("singleDestination");
    expect(request?.mode).toBe("singleDestination");
    expect(request?.entries).toEqual([
      {
        source_profile_paths: ["C:/browser/You"],
        source_profile_names: ["You"],
        destination_kind: "existing",
        destination_name: "Default",
        destination_profile_id: "default",
      },
    ]);
  });

  test("additional data selection widens the import scope to everything", () => {
    expect(
      browserImportScopeFromSelection({
        cookies: false,
        history: false,
        additionalData: true,
      }),
    ).toBe("everything");
  });

  test("duplicate source names get stable create names", () => {
    const request = buildBrowserImportStartRequest(
      [profile("Work", "C:/one"), profile("Work", "C:/two")],
      {
        scope: "cookiesAndHistory",
      },
    );

    expect(request?.entries.map((entry) => entry.destination_name)).toEqual([
      "Work",
      "Work (2)",
    ]);
  });

  test("separate mode can target an existing destination per source profile", () => {
    const request = buildBrowserImportStartRequest([profile("You")], {
      mode: "separateProfiles",
      scope: "cookiesAndHistory",
      destinationProfiles: [{ id: "default", display_name: "Default", is_default: true }],
      separateDestinationChoices: {
        "C:/browser/You": { kind: "existing", destinationProfileId: "default" },
      },
    });

    expect(request?.entries).toEqual([
      {
        source_profile_paths: ["C:/browser/You"],
        source_profile_names: ["You"],
        destination_kind: "existing",
        destination_name: "Default",
        destination_profile_id: "default",
      },
    ]);
  });

  test("normalizes empty destination fixtures back to the default profile", () => {
    expect(normalizedDestinationProfiles([])).toEqual([
      { id: "default", display_name: "Default", is_default: true },
    ]);
    expect(
      normalizedDestinationProfiles([
        { id: " work ", display_name: " Work ", is_default: true },
      ]),
    ).toEqual([{ id: "work", display_name: "Work", is_default: true }]);
  });
});
