integer gCount;
float gScale = 1.5;
vector gOffset = <0.0, 1.0, 0.0>;

default
{
    state_entry()
    {
        integer value = (integer)gScale;
        float converted = (float)value;
        string text = (string)converted;
        key owner = (key)llGetOwner();
        gCount = value + 2;
        gCount *= 2;
        gOffset.x = converted;
        gOffset.y += 0.5;
        llOwnerSay(text);
        llOwnerSay((string)owner);
    }
}
