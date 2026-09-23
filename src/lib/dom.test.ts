import { describe, expect, it, vi } from "vitest";

// dom.ts 的运行时判定依赖全局 HTMLElement（instanceof）；vitest 运行在 node 环境，
// 先桩一个同名类再动态导入，使 instanceof 语义与浏览器一致（format.i18n.test.ts 同惯例）。
class FakeHTMLElement {}
vi.stubGlobal("HTMLElement", FakeHTMLElement);

const { isEditableTarget } = await import("./dom");

function element(tag: string, options: { contentEditable?: boolean } = {}) {
  const el = Object.create(FakeHTMLElement.prototype) as unknown as HTMLElement;
  const tagName = tag.toUpperCase();
  const editable =
    options.contentEditable === true || ["INPUT", "TEXTAREA", "SELECT"].includes(tagName);
  Object.defineProperty(el, "tagName", { value: tagName, configurable: true });
  if (options.contentEditable) {
    Object.defineProperty(el, "isContentEditable", { value: true, configurable: true });
  }
  el.closest = (selector: string) =>
    selector === "input, textarea, select, [contenteditable='true']" && editable ? el : null;
  return el;
}

describe("isEditableTarget", () => {
  it("非 HTMLElement 返回 false", () => {
    expect(isEditableTarget(null)).toBe(false);
    expect(isEditableTarget({} as EventTarget)).toBe(false);
  });
  it("input/textarea/select/contenteditable 命中", () => {
    expect(isEditableTarget(element("input"))).toBe(true);
    expect(isEditableTarget(element("textarea"))).toBe(true);
    expect(isEditableTarget(element("select"))).toBe(true);
    expect(isEditableTarget(element("div", { contentEditable: true }))).toBe(true);
  });
  it("普通元素返回 false", () => {
    expect(isEditableTarget(element("div"))).toBe(false);
  });
});
