/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use font::{Font, FontDescriptor, FontGroup, FontStyle, SelectorPlatformIdentifier};
use font::{SpecifiedFontStyle, UsedFontStyle};
use font_list::FontList;
use servo_util::cache::Cache;
use servo_util::cache::MonoCache;
use servo_util::time::ProfilerChan;

use platform::font::FontHandle;
use platform::font_context::FontContextHandle;

use azure::azure_hl::BackendType;
use core::hashmap::HashMap;
use core::io::println;

// FIXME(seth): This probably should not live here.
pub struct LRUCache<K, V> {
    entries: ~[(K, V)],
    cache_size: uint,
}

pub impl<K: Eq + Copy, V: Copy> LRUCache<K, V> {
    fn new(cache_size: uint) -> LRUCache<K, V> {
        LRUCache {
          entries: ~[],
          cache_size: cache_size,
        }
    }

    fn lookup(&mut self, key: &K) -> Option<V> {
        match self.entries.position(|&(k, _)| k == *key) {
            Some(pos) => Some(self.touch(pos)),
            None      => None,
        }
    }

    // FIXME(seth): I'm told this won't work til we update rust. Making insert public
    // for now.
    fn lookup_or_insert(&mut self, key: &K, f: &fn(&K) -> V) -> V {
        match self.entries.position(|&(k, _)| k == *key) {
            Some(pos) => self.touch(pos),
            None      => self.insert(key, f(key)),
        }
    }

    fn insert(&mut self, key: &K, val: V) -> V {
        // There's been a cache miss. Insert the new value into the cache.
        println("FONT GROUP CACHE MISS");
        if self.entries.len() == self.cache_size {
            self.entries.remove(0);
        }
        self.entries.push((*key, copy val));
        val
    }

    priv fn touch(&mut self, pos: uint) -> V {
        // There's been a cache hit. Update the value's position.
        println("FONT GROUP CACHE HIT");
        let (key, val) = copy self.entries[pos];
        if pos != self.cache_size {
            self.entries.remove(pos);
            self.entries.push((key, copy val));
        }
        val
    }
}

// TODO(Rust #3934): creating lots of new dummy styles is a workaround
// for not being able to store symbolic enums in top-level constants.
pub fn dummy_style() -> FontStyle {
    use font::FontWeight300;
    return FontStyle {
        pt_size: 20f,
        weight: FontWeight300,
        italic: false,
        oblique: false,
        families: ~"serif, sans-serif",
    }
}

pub trait FontContextHandleMethods {
    fn clone(&self) -> FontContextHandle;
    fn create_font_from_identifier(&self, ~str, UsedFontStyle) -> Result<FontHandle, ()>;
}

#[allow(non_implicitly_copyable_typarams)]
pub struct FontContext {
    instance_cache: MonoCache<FontDescriptor, @mut Font>,
    font_list: Option<FontList>, // only needed by layout
    font_groups: LRUCache<SpecifiedFontStyle, @FontGroup>,
    handle: FontContextHandle,
    backend: BackendType,
    generic_fonts: HashMap<~str,~str>,
    profiler_chan: ProfilerChan,
}

#[allow(non_implicitly_copyable_typarams)]
pub impl<'self> FontContext {
    fn new(backend: BackendType,
           needs_font_list: bool,
           profiler_chan: ProfilerChan)
           -> FontContext {
        let handle = FontContextHandle::new();
        let font_list = if needs_font_list { 
                            Some(FontList::new(&handle, profiler_chan.clone())) }
                        else { None };

        // TODO: Allow users to specify these.
        let mut generic_fonts = HashMap::with_capacity(5);
        generic_fonts.insert(~"serif", ~"Times New Roman");
        generic_fonts.insert(~"sans-serif", ~"Arial");
        generic_fonts.insert(~"cursive", ~"Apple Chancery");
        generic_fonts.insert(~"fantasy", ~"Papyrus");
        generic_fonts.insert(~"monospace", ~"Menlo");

        FontContext { 
            // TODO(Rust #3902): remove extraneous type parameters once they are inferred correctly.
            instance_cache:
                Cache::new::<FontDescriptor,@mut Font,MonoCache<FontDescriptor,@mut Font>>(10),
            font_list: font_list,
            font_groups: LRUCache::new(5),
            handle: handle,
            backend: backend,
            generic_fonts: generic_fonts,
            profiler_chan: profiler_chan,
        }
    }

    priv fn get_font_list(&'self self) -> &'self FontList {
        self.font_list.get_ref()
    }

    fn get_resolved_font_for_style(&mut self, style: &SpecifiedFontStyle) -> @FontGroup {
        match self.font_groups.lookup(style) {
            Some(fg) => fg,
            None => {
                let fg = self.create_font_group(style);
                self.font_groups.insert(style, fg)
            }
        }
        // FIXME(seth): Once lookup_or_insert works, (i.e. once rustc is
        // updated) we should use it.
        //self.font_groups.lookup_or_insert(style, |s| self.create_font_group(s))
    }

    fn get_font_by_descriptor(&mut self, desc: &FontDescriptor) -> Result<@mut Font, ()> {
        match self.instance_cache.find(desc) {
            Some(f) => Ok(f),
            None => { 
                let result = self.create_font_instance(desc);
                match result {
                    Ok(font) => {
                        self.instance_cache.insert(desc, font);
                    }, _ => {}
                };
                result
            }
        }
    }

    priv fn transform_family(&self, family: &str) -> ~str {
        // FIXME: Need a find_like() in HashMap.
        let family = family.to_str();
        debug!("(transform family) searching for `%s`", family);
        match self.generic_fonts.find(&family) {
            None => family,
            Some(mapped_family) => copy *mapped_family
        }
    }

    // TODO:(Issue #196): cache font groups on the font context.
    priv fn create_font_group(&mut self, style: &SpecifiedFontStyle) -> @FontGroup {
        let mut fonts = ~[];

        debug!("(create font group) --- starting ---");

        // TODO(Issue #193): make iteration over 'font-family' more robust.
        for str::each_split_char(style.families, ',') |family| {
            let family_name = str::trim(family);
            let transformed_family_name = self.transform_family(family_name);
            debug!("(create font group) transformed family is `%s`", transformed_family_name);

            let list = self.get_font_list();

            let result = list.find_font_in_family(transformed_family_name, style);
            let mut found = false;
            for result.each |font_entry| {
                found = true;
                // TODO(Issue #203): route this instantion through FontContext's Font instance cache.
                let instance = Font::new_from_existing_handle(self, &font_entry.handle, style, self.backend,
                                                              self.profiler_chan.clone());
                do result::iter(&instance) |font: &@mut Font| { fonts.push(*font); }
            };

            if !found {
                debug!("(create font group) didn't find `%s`", transformed_family_name);
            }
        }

        assert!(fonts.len() > 0);
        // TODO(Issue #179): Split FontStyle into specified and used styles
        let used_style = copy *style;

        debug!("(create font group) --- finished ---");

        @FontGroup::new(style.families.to_managed(), &used_style, fonts)
    }

    priv fn create_font_instance(&self, desc: &FontDescriptor) -> Result<@mut Font, ()> {
        return match &desc.selector {
            // TODO(Issue #174): implement by-platform-name font selectors.
            &SelectorPlatformIdentifier(ref identifier) => { 
                let result_handle = self.handle.create_font_from_identifier(copy *identifier,
                                                                            copy desc.style);
                result::chain(result_handle, |handle| {
                    Ok(Font::new_from_adopted_handle(self,
                                                     handle,
                                                     &desc.style,
                                                     self.backend,
                                                     self.profiler_chan.clone()))
                })
            }
        };
    }
}
