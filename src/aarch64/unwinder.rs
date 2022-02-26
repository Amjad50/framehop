use std::ops::Deref;

use crate::{
    unwinder::UnwinderInternal, AllocationPolicy, Error, FrameAddress, MayAllocateDuringUnwind,
    Module, Unwinder,
};

use super::{ArchAarch64, CacheAarch64, UnwindRegsAarch64};

pub struct UnwinderAarch64<
    D: Deref<Target = [u8]>,
    P: AllocationPolicy<D> = MayAllocateDuringUnwind,
>(UnwinderInternal<D, ArchAarch64, P>);

impl<D: Deref<Target = [u8]>, P: AllocationPolicy<D>> Default for UnwinderAarch64<D, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D: Deref<Target = [u8]>, P: AllocationPolicy<D>> UnwinderAarch64<D, P> {
    pub fn new() -> Self {
        Self(UnwinderInternal::new())
    }
}

impl<D: Deref<Target = [u8]>, P: AllocationPolicy<D>> Unwinder for UnwinderAarch64<D, P> {
    type UnwindRegs = UnwindRegsAarch64;
    type Cache = CacheAarch64<D, P>;
    type Module = Module<D>;

    fn add_module(&mut self, module: Module<D>) {
        self.0.add_module(module);
    }

    fn remove_module(&mut self, module_address_range_start: u64) {
        self.0.remove_module(module_address_range_start);
    }

    fn unwind_frame<F>(
        &self,
        address: FrameAddress,
        regs: &mut UnwindRegsAarch64,
        cache: &mut CacheAarch64<D, P>,
        read_mem: &mut F,
    ) -> Result<Option<u64>, Error>
    where
        F: FnMut(u64) -> Result<u64, ()>,
    {
        self.0.unwind_frame(address, regs, &mut cache.0, read_mem)
    }
}
