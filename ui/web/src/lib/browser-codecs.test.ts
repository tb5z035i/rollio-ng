import { describe, expect, it } from "vitest";
import { videoDecoderAvailability } from "./browser-codecs";

describe("videoDecoderAvailability", () => {
  it("reports ready when VideoDecoder is exposed", () => {
    expect(
      videoDecoderAvailability({
        VideoDecoder: class {},
        isSecureContext: true,
        location: { protocol: "http:", host: "127.0.0.1:3000" },
      }),
    ).toMatchObject({
      available: true,
      summary: "WebCodecs ready",
    });
  });

  it("explains insecure remote origins", () => {
    const availability = videoDecoderAvailability({
      isSecureContext: false,
      location: { protocol: "http:", host: "192.168.1.10:3000" },
    });

    expect(availability.available).toBe(false);
    expect(availability.detail).toContain("not secure");
    expect(availability.detail).toContain("localhost/127.0.0.1");
  });

  it("explains browsers that do not expose WebCodecs", () => {
    const availability = videoDecoderAvailability({
      isSecureContext: true,
      location: { protocol: "https:", host: "rollio.local" },
    });

    expect(availability.available).toBe(false);
    expect(availability.detail).toContain("does not expose it");
    expect(availability.detail).toContain("WebCodecs-capable browser");
  });
});
