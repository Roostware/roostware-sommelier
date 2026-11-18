<script lang="ts">
  import { onDestroy } from "svelte";
  import ProblemWorkspace from "$lib/ProblemWorkspace.svelte";
  import ProblemPicker from "$lib/ProblemPicker.svelte";
  import { resetIfSolved } from "$lib/sittingEnd";
  import { closeProblem, solve } from "$lib/solve.svelte";

  // Leaving the tab after solving a problem starts the next visit clean (see resetIfSolved), and
  // the problem closes, back to the picker. Leaving mid-attempt keeps everything as it was.
  onDestroy(() => {
    if (resetIfSolved()) closeProblem();
  });
</script>

<!-- With a problem open the tab is about that problem; otherwise it's a way to pick one. -->
{#if solve.problemId === null}
  <ProblemPicker />
{:else}
  <ProblemWorkspace />
{/if}
