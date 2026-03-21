import { create } from "zustand";
import { persist } from "zustand/middleware";

interface AccountStore {
  selectedAccountId: string | null;
  setSelectedAccountId: (id: string | null) => void;
}

export const useAccountStore = create<AccountStore>()(
  persist(
    (set) => ({
      selectedAccountId: null,
      setSelectedAccountId: (id) => set({ selectedAccountId: id }),
    }),
    {
      name: "scrapix-account",
    }
  )
);
