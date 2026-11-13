<script lang="ts">
  import ProblemWorkspace from "$lib/ProblemWorkspace.svelte";
  import ProblemPicker from "$lib/ProblemPicker.svelte";
  import { solve } from "$lib/solve.svelte";
</script>

<!-- With a problem open the tab is about that problem; otherwise it's a way to pick one. -->
{#if solve.problemId === null}
  <ProblemPicker />
{:else}
  <ProblemWorkspace />
{/if}
