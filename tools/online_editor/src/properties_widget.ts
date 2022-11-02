// Copyright © SixtyFPS GmbH <info@slint-ui.com>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-commercial

// cSpell: ignore lumino

import { GotoPositionCallback, ReplaceTextFunction, TextRange } from "./text";
import {
    lsp_range_to_editor_range,
    DefinitionPosition,
} from "./lsp_integration";

import { Message } from "@lumino/messaging";
import { Widget } from "@lumino/widgets";

import {
    BindingTextProvider,
    Element,
    Property,
    PropertyQuery,
} from "./lsp_integration";

const TYPE_PATTERN = /^[a-z-]+$/i;

let FOCUSING = false;

function type_class_for_typename(name: string): string {
    if (name === "callback" || name.slice(0, 9) === "callback(") {
        return "type-callback";
    }
    if (name.slice(0, 5) === "enum ") {
        return "type-enum";
    }
    if (name.slice(0, 9) === "function(") {
        return "type-function";
    }
    if (name === "element ref") {
        return "type-element-ref";
    }
    if (TYPE_PATTERN.test(name)) {
        return "type-" + name;
    }
    return "type-unknown";
}

function editor_definition_range(
    uri: string,
    def_pos: DefinitionPosition | null,
): TextRange | null {
    if (def_pos == null) {
        return null;
    }
    return lsp_range_to_editor_range(uri, def_pos.expression_range);
}

export class PropertiesWidget extends Widget {
    #onGotoPosition: GotoPositionCallback = (_u, _p) => {
        return;
    };
    #replaceText: ReplaceTextFunction = (_u, _r, _t, _v) => {
        return true;
    };

    static createNode(): HTMLElement {
        const node = document.createElement("div");
        const content = document.createElement("div");
        node.appendChild(content);

        const header = document.createElement("div");
        header.className = "element-header";
        const element_type = document.createElement("div");
        element_type.className = "element-type";
        const element_id = document.createElement("div");
        element_id.className = "element-id";
        header.appendChild(element_type);
        header.appendChild(element_id);

        const table = document.createElement("table");
        table.className = "properties-table";

        content.appendChild(header);
        content.appendChild(table);

        return node;
    }

    constructor() {
        super({ node: PropertiesWidget.createNode() });
        this.setFlag(Widget.Flag.DisallowLayout);
        this.addClass("content");
        this.addClass("properties-editor".toLowerCase());
        this.title.label = "Properties";
        this.title.closable = true;
        this.title.caption = `Element Properties`;

        this.set_header(null);
    }

    protected onCloseRequest(msg: Message): void {
        super.onCloseRequest(msg);
        this.dispose();
    }

    set on_goto_position(callback: GotoPositionCallback) {
        this.#onGotoPosition = callback;
    }

    set replace_text_function(fn: ReplaceTextFunction) {
        this.#replaceText = fn;
    }

    protected get contentNode(): HTMLDivElement {
        return this.node.getElementsByTagName("div")[0] as HTMLDivElement;
    }
    protected get headerNode(): HTMLDivElement {
        return this.contentNode.getElementsByTagName(
            "div",
        )[0] as HTMLDivElement;
    }
    protected get elementTypeNode(): HTMLDivElement {
        return this.headerNode.getElementsByTagName("div")[0] as HTMLDivElement;
    }
    protected get elementIdNode(): HTMLDivElement {
        return this.headerNode.getElementsByTagName("div")[1] as HTMLDivElement;
    }
    protected get tableNode(): HTMLTableElement {
        return this.contentNode.getElementsByTagName(
            "table",
        )[0] as HTMLTableElement;
    }

    private set_header(element: Element | null) {
        if (element == null) {
            this.elementTypeNode.innerText = "<Unknown>";
            this.elementIdNode.innerText = "";
        } else {
            this.elementTypeNode.innerText = element.type_name;
            this.elementIdNode.innerText = element.id;
        }
    }

    private replace_property_value(
        uri: string,
        range: TextRange,
        new_value: string,
        validator: (_old: string) => boolean,
    ): boolean {
        return this.#replaceText(uri, range, new_value, validator);
    }

    private populate_table(
        binding_text_provider: BindingTextProvider,
        properties: Property[],
        uri: string,
    ) {
        const table = this.tableNode;

        let current_group = "";

        table.innerHTML = "";

        for (const p of properties) {
            if (p.group !== current_group) {
                const group_header = document.createElement("tr");
                group_header.className = "group-header";

                const group_cell = document.createElement("td");
                group_cell.innerText = p.group;
                group_cell.setAttribute("colspan", "2");
                current_group = p.group;

                group_header.appendChild(group_cell);
                table.appendChild(group_header);
            }
            const row = document.createElement("tr");
            row.className = "property";
            if (p.declared_at == null) {
                row.classList.add("builtin");
            }
            if (p.defined_at == null) {
                row.classList.add("undefined");
            }

            const expression_range = editor_definition_range(uri, p.defined_at);

            const goto_property = () => {
                if (expression_range != null) {
                    this.#onGotoPosition(uri, expression_range);
                }
            };

            const name_field = document.createElement("td");
            name_field.className = "name-column";
            name_field.innerText = p.name;
            name_field.addEventListener("click", goto_property);
            row.appendChild(name_field);

            const value_field = document.createElement("td");
            value_field.className = "value-column";
            value_field.classList.add(type_class_for_typename(p.type_name));
            value_field.setAttribute("title", p.type_name);
            const input = document.createElement("input");
            input.type = "text";
            if (p.defined_at != null) {
                const code_text = binding_text_provider.binding_text(
                    p.defined_at,
                );
                input.value = code_text;
                const changed_class = "value-changed";
                input.addEventListener("focus", (_) => {
                    if (FOCUSING) {
                        FOCUSING = false;
                    } else {
                        FOCUSING = true;
                        goto_property();
                        input.focus();
                    }
                });
                input.addEventListener("input", (_) => {
                    const current_text = input.value;
                    if (current_text != code_text) {
                        input.classList.add(changed_class);
                    } else {
                        input.classList.remove(changed_class);
                    }
                });
                input.addEventListener("change", (_) => {
                    const current_text = input.value;
                    if (current_text != code_text && expression_range != null) {
                        this.replace_property_value(
                            uri,
                            expression_range,
                            current_text,
                            (old_text) => old_text == code_text,
                        );
                    }
                });
            } else {
                input.disabled = true;
            }
            value_field.appendChild(input);
            row.appendChild(value_field);

            table.appendChild(row);
        }
    }

    set_properties(
        binding_text_provider: BindingTextProvider,
        properties: PropertyQuery,
    ) {
        this.set_header(properties.element);
        this.populate_table(
            binding_text_provider,
            properties.properties,
            properties.source_uri,
        );
    }
}