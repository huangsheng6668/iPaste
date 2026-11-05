import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import CategoryContextMenu from "./CategoryContextMenu.vue";
import { SEND_TARGET_ALL, type SendTarget } from "../../lib/deviceDisplay";

const targets: SendTarget[] = [
  { id: SEND_TARGET_ALL, name: null, isAll: true },
  { id: "b".repeat(64), name: "MacBook Pro", isAll: false },
];

function mountMenu(sendTargets: SendTarget[]) {
  return mount(CategoryContextMenu, {
    props: {
      menu: { left: 16, top: 16 },
      isPendingDelete: false,
      sendTargets,
    },
  });
}

describe("CategoryContextMenu 整组发送入口", () => {
  it("无在线设备时只显示空态提示", () => {
    const wrapper = mountMenu([]);
    expect(wrapper.findAll(".category-context-item")).toHaveLength(3); // 重命名/改色/删除
    expect(wrapper.find(".category-context-empty").exists()).toBe(true);
    wrapper.unmount();
  });

  it("按目标设备渲染条目并上抛 targetId", async () => {
    const wrapper = mountMenu(targets);
    const items = wrapper.findAll(".category-context-item");
    // 3 个分类动作 + 2 个发送目标
    expect(items).toHaveLength(5);
    expect(wrapper.find(".category-context-empty").exists()).toBe(false);

    // 顺序：重命名、改色、发送到全部在线、发送到指定设备、删除
    await items[2]?.trigger("click");
    expect(wrapper.emitted("sendCategory")?.[0]).toEqual([SEND_TARGET_ALL]);

    await items[3]?.trigger("click");
    expect(wrapper.emitted("sendCategory")?.[1]).toEqual([targets[1]?.id]);
    wrapper.unmount();
  });
});
