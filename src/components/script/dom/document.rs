/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use dom::bindings::document;
use dom::bindings::utils::{DOMString, WrapperCache};
use dom::htmlcollection::HTMLCollection;
use dom::node::{AbstractNode, ScriptView};
use dom::window::Window;
use script_task::global_script_context;

use js::jsapi::bindgen::{JS_AddObjectRoot, JS_RemoveObjectRoot};
use servo_util::tree::{TreeNodeRef, TreeUtils};

pub struct Document {
    root: AbstractNode<ScriptView>,
    wrapper: WrapperCache,
    window: Option<@mut Window>,
}

pub fn Document(root: AbstractNode<ScriptView>, window: Option<@mut Window>) -> @mut Document {
    let doc = @mut Document {
        root: root,
        wrapper: WrapperCache::new(),
        window: window
    };
    let compartment = global_script_context().js_compartment;
    do root.with_base |base| {
        assert!(base.wrapper.get_wrapper().is_not_null());
        let rootable = base.wrapper.get_rootable();
        JS_AddObjectRoot(compartment.cx.ptr, rootable);
    }
    document::create(compartment, doc);
    doc
}

pub impl Document {
    fn getElementsByTagName(&self, tag: DOMString) -> Option<@mut HTMLCollection> {
        let mut elements = ~[];
        let tag = tag.to_str();
        let _ = for self.root.traverse_preorder |child| {
            if child.is_element() {
                do child.with_imm_element |elem| {
                    if elem.tag_name == tag {
                        elements.push(child);
                    }
                }
            }
        };
        Some(HTMLCollection::new(elements))
    }

    fn content_changed(&self) {
        for self.window.each |window| {
            window.content_changed()
        }
    }

    fn teardown(&self) {
        let compartment = global_script_context().js_compartment;
        do self.root.with_base |node| {
            assert!(node.wrapper.get_wrapper().is_not_null());
            let rootable = node.wrapper.get_rootable();
            JS_RemoveObjectRoot(compartment.cx.ptr, rootable);
        }
    }
}

