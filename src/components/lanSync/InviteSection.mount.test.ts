import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import InviteSection from "./InviteSection.vue";

function mountSection(fingerprint: string | null, ticket: string | null = "ipaste-pair-v1:x") {
  return mount(InviteSection, {
    props: {
      pin: "123456",
      ticket,
      fingerprint,
      countdown: "10:00",
      expired: false,
      creating: false,
      error: null,
      copied: null,
    },
  });
}

describe("InviteSection 本机指纹行", () => {
  it("有指纹时渲染指纹串与 label", () => {
    const wrapper = mountSection("ab12cd34");
    const rows = wrapper.findAll(".lan-pin-row");
    // PIN 行 + 指纹行
    expect(rows).toHaveLength(2);
    expect(rows[1]?.text()).toContain("ab12cd34");
    expect(rows[1]?.text()).toContain("This device's code");
    wrapper.unmount();
  });

  it("指纹为 null 时不渲染该行", () => {
    const wrapper = mountSection(null);
    // 仅 PIN 行
    expect(wrapper.findAll(".lan-pin-row")).toHaveLength(1);
    expect(wrapper.text()).not.toContain("This device's code");
    wrapper.unmount();
  });

  it("无邀请（ticket=null）时不渲染指纹行", () => {
    const wrapper = mountSection("ab12cd34", null);
    expect(wrapper.findAll(".lan-pin-row")).toHaveLength(0);
    wrapper.unmount();
  });
});
