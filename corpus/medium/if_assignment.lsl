key target;

default
{
    state_entry()
    {
        // Likely typo: assignment instead of equality comparison.
        if (target = llGetOwner())
        {
            llOwnerSay("this condition was probably meant to compare");
        }

        // Extra parentheses explicitly mark this assignment as intentional.
        if ((target = llGetOwner()))
        {
            llOwnerSay("intentional assignment condition");
        }

        if (target == llGetOwner())
        {
            llOwnerSay("ordinary comparison");
        }
    }
}
